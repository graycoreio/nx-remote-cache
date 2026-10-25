# nx-remote-cache

A [Nx](https://nx.dev) remote cache server, written in Rust and backed by Azure Blob Storage, that runs **next to whoever uses it**: inside each CI job, started by [the nx-remote-cache action](https://github.com/graycoreio/github-actions/tree/main/nx-remote-cache), or on a developer's machine. There is no hosted service to run. Each instance talks to the shared Blob container with its caller's own Azure identity.

It implements Nx's [remote cache OpenAPI spec](https://nx.dev/docs/kb/self-hosted-caching) (vendored in [`spec/`](spec/nx-remote-cache.openapi.json)). Nx 20.8+ ships its own client for that spec, so a workspace only needs two environment variables:

```sh
export NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://127.0.0.1:<port>
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=<token>
```

It replaces `@nx/azure-cache` (and the other `@nx/*-cache` packages). Nx deprecated those because of [CVE-2025-36852 (CREEP)](https://nx.dev/docs/reference/deprecated/self-hosted-cache-packages): any job holding the single read/write credential could overwrite cache entries. Here:

- **Writing needs a separate, narrow identity.** CI jobs exchange their GitHub OIDC token for one of two Azure identities. Only the writer can write to the container, and Azure only issues it to the subjects you list, normally pushes to the default branch. Pull requests get the reader.
- **Entries aren't overwritten through the server.** Every upload is an atomic create-if-absent; a second upload for the same hash gets `409`. A job holding the writer identity could bypass the server, though; see [Security model](#security-model).

## Tested Nx versions

The server is verified end to end against real Nx clients:

| Nx | Status | Scenarios run |
|---|---|---|
| 23.3.0 | Tested | All of the below |
| 24.x | Not yet tested | |

Full scenario list: write, then remote hit with byte-identical output; read-only token (upload skipped, run passes, including large artifacts); invalid tokens (run fails); CI writer and reader identities through the action; a reader-identity server configured as writable (storage refuses the write, Nx skips it); a server URL with a trailing slash.

## Install

Every [release](https://github.com/graycoreio/nx-remote-cache/releases) after v0.2.0 publishes:

- **Binaries:** `nx-remote-cache-<version>-<target>.tar.gz` for
  - `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` (glibc 2.34+: Ubuntu 22.04, Debian 12, RHEL 9 or newer),
  - `universal-apple-darwin` (one binary for Apple Silicon and Intel Macs).

  Each release has a `SHA256SUMS` file.
- **Images:** `ghcr.io/graycoreio/nx-remote-cache:<version>` for `linux/amd64` and `linux/arm64`. The image contains the same binary as the Linux archives.

```sh
version=<version>
target=universal-apple-darwin    # or x86_64-unknown-linux-gnu / aarch64-unknown-linux-gnu
archive="nx-remote-cache-$version-$target.tar.gz"
gh release download "v$version" --repo graycoreio/nx-remote-cache --pattern "$archive" --pattern SHA256SUMS
grep " $archive\$" SHA256SUMS | shasum -a 256 -c -
tar -xzf "$archive" nx-remote-cache && sudo install nx-remote-cache /usr/local/bin/
```

The macOS binary isn't notarised. Downloaded with `gh` or `curl` it runs as is; if you download it in a browser, macOS blocks it until you run `xattr -d com.apple.quarantine nx-remote-cache`.

## How it fits together

```
 CI job / dev machine
   Nx ── static token ──▶ nx-remote-cache (127.0.0.1) ── the caller's Azure identity ──▶ Blob container
```

**Client → server:** static tokens (`[[auth.providers]] type = "static"`), configured as SHA-256 hashes. Whoever starts the server makes one up and hands it to Nx: the action generates a random token per job, the dev compose setup uses a well-known one on `127.0.0.1`. Nobody else can reach the server, so the token only keeps other local processes out. Its scope (`read` or `read_write`) turns uploads from read-only callers into a clean `403`, which Nx skips.

**Server → storage** uses exactly one explicitly configured mode. There is no credential chain guessing what to use.

| Where it runs | `storage.auth.mode` |
|---|---|
| A CI job (the action) | `workload_identity`: the job's GitHub OIDC token, exchanged for the writer or reader identity |
| Docker on a dev machine | `managed_identity` through the `az` sidecar ([Local instance](#local-instance-binary-or-docker)), or `sas`: [`scripts/dev-sas.sh`](scripts/dev-sas.sh) prints a short-lived user-delegation SAS (needs Storage Blob Delegator on the account, which `local.bicep` grants its developer groups) |
| The binary or `cargo run` on a dev machine | `developer_tools`: your `az login` |
| Elsewhere | `client_secret` (a service principal) |
| Azurite | `sas` with `endpoint = "http://…:10000/devstoreaccount1"` |

The identity's role on the container (Storage Blob Data Reader or Contributor) decides what it can do. Shared keys and connection strings are deliberately not supported.

### Behaviour worth knowing

- **Any rejected token fails the Nx run.** Nx treats a `401` from the cache as fatal, even after every task succeeded, so a wrong token shows up immediately. A read-only token's `403` on upload is *not* fatal; Nx just skips the upload. Rejected uploads read the rest of the body first, so Nx sees that `403` rather than a broken connection.
- **Uploads stream.** Large uploads are staged in 8 MiB blocks and committed with `If-None-Match: *`. Each upload uses about 30 MB; each download about 90 MB, regardless of artifact size.
- **Provenance is recorded.** Every blob carries the name of the static token that wrote it (`ci-job` for the action, `dev-<username>` for the dev compose setup).
- **Startup fails fast.** The server checks storage access at startup and exits with an explanation (missing container, missing role assignment, expired SAS, …) instead of serving cache misses.
- **`/health` answers once the server is usable.** The storage check happens before it starts listening, and it exits instead if storage fails, so there's no separate readiness probe.

## Configuration

The server reads one TOML document, assembled from up to three sources (later ones win, and all of them win over the `NX_CACHE_DEFAULT_LISTEN` default):

1. **A file:** `config.toml` in the working directory, or `--config <path>`. The default file is optional when another source supplies the config; a path given with `--config` must exist.
2. **Inline TOML** in the `NX_CACHE_CONFIG_TOML` environment variable. Handy where mounting a file is awkward; this is how the action passes its config.
3. **Per-key overrides** `NX_CACHE__<SECTION>__<KEY>`, e.g. `NX_CACHE__SERVER__LISTEN=127.0.0.1:8080` or `NX_CACHE__STORAGE__CONTAINER=nx-cache-v2`. These work for single values. Lists such as `[[auth.providers]]` can only be set in the file or the inline TOML.

Config never contains secrets: SAS tokens and client secrets are read from environment variables that the config *names*. Static tokens are stored as SHA-256 hashes.

### Where the config goes

| How you run it | Where the config lives |
|---|---|
| `cargo run` / the binary | `./config.toml` in the working directory, or `--config <path>` |
| Docker image | Mount it at `/etc/nx-remote-cache/config.toml` (the image's working directory), or pass `NX_CACHE_CONFIG_TOML` |
| `docker compose` (this repo) | [`config/local.toml`](config/local.toml), mounted as above |
| The action | Generated per job and passed as `NX_CACHE_CONFIG_TOML` |

Examples: [`config/local.toml`](config/local.toml) (Azurite, well-known local tokens) and [`config/developer.example.toml`](config/developer.example.toml) (your `az login`, real storage).

```sh
nx-remote-cache generate-token        # new random static token + the sha256 to put in config
nx-remote-cache hash-token <token>    # sha256 of an existing token
```

### Reference

Unknown keys are rejected at startup, so a typo fails loudly rather than being ignored.

**`[server]`** (optional)

| Key | Default | |
|---|---|---|
| `listen` | `127.0.0.1:3000` | Address and port. `NX_CACHE_DEFAULT_LISTEN` replaces this default without overriding an explicit `listen`; the image sets it to `0.0.0.0:3000`, so publish the port on the host's loopback: `-p 127.0.0.1:3000:3000` |
| `max_artifact_bytes` | `2147483648` (2 GiB) | Larger uploads get `413` |
| `json_logs` | `false` | JSON log lines, for log aggregation |

Log verbosity follows `RUST_LOG` (default `info`).

**`[storage]`**: `type = "azure"`, or `type = "memory"` (not persistent; local smoke tests only).

| Key (`azure`) | Default | |
|---|---|---|
| `account` | — | Storage account name. One of `account` / `endpoint` is required |
| `endpoint` | — | Full blob service URL instead of `account`, e.g. Azurite's `http://127.0.0.1:10000/devstoreaccount1` |
| `container` | required | Must exist; the server never creates it |
| `prefix` | `v1` | Virtual directory for entries inside the container |
| `block_size` | `8388608` (8 MiB) | Uploads up to this size go up in one request; larger ones in blocks of this size |
| `download_parallelism` | `4` | Concurrent range requests per download; each in-flight download buffers up to ~90 MiB at the default |
| `auth` | required | Exactly one mode, as an inline table, below |

| `auth.mode` | Other keys | |
|---|---|---|
| `managed_identity` | `client_id` (omit for system-assigned) | An Azure managed-identity endpoint, or the dev compose sidecar that emulates one |
| `workload_identity` | `tenant_id`, `client_id` (both default to `AZURE_TENANT_ID` / `AZURE_CLIENT_ID`) | Federation via `AZURE_FEDERATED_TOKEN_FILE`; the action uses it with the job's GitHub OIDC token |
| `client_secret` | `tenant_id`, `client_id`, `client_secret_env` (default `AZURE_CLIENT_SECRET`) | A service principal |
| `developer_tools` | — | Your `az login` / `azd auth login` |
| `sas` | `sas_token_env` (default `AZURE_STORAGE_SAS_TOKEN`) | The only mode that works over plain `http` (Azurite) |

**`[[auth.providers]]`**: at least one is required.

| `type` | Keys |
|---|---|
| `static` | `tokens = [{ name, sha256, scope }]`, where `scope` is `read` or `read_write`. `name` is recorded on everything the token writes |

## Local development

```sh
docker compose up --build             # Azurite + server on :3000
export NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://localhost:3000
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=local-write-token   # or local-read-token
```

To run against real Azure storage from a dev machine, set `auth = { mode = "developer_tools" }` and run `cargo run -- --config <file>` after `az login`.

Tests:

```sh
cargo test                     # unit, HTTP API, spec conformance
scripts/azurite-test.sh        # everything, including the Azure store against Azurite
```

[`.github/workflows/e2e.yml`](.github/workflows/e2e.yml) runs the whole flow through the action from GitHub, with the server built from the commit (writer on `main`, reader on PRs). It doubles as a worked example of a consumer workflow; it needs the `NX_CACHE_E2E_*` repository variables listed in its header.

## Deploying to Azure

[`deploy/local.bicep`](deploy/local.bicep) creates what server instances need, wherever they run:

- storage: either a **new** account (`createStorageAccount = true`: no shared keys, no public access, TLS 1.2 or later), or an **existing** one, in the same or another resource group (`storageResourceGroup`). Either way it adds the cache container (unless `createContainer = false`) and an optional lifecycle (expiry) rule;
- two CI managed identities with GitHub federated credentials: the writer gets **Storage Blob Data Contributor** on the container, the reader **Storage Blob Data Reader**;
- optionally, the same roles for developer groups (`readerGroupObjectIds` / `writerGroupObjectIds`), plus Storage Blob Delegator on the account so they can sign user-delegation SAS tokens.

Deploy it as a **deployment stack**, so that removing a subject or group from the parameters also deletes its credential or role assignment:

```sh
az stack group create -n <stack name> -g <resource group> -f deploy/local.bicep \
  -p storageAccountName=<account> writerSubjects='["<subject>"]' readerSubjects='["<subject>"]' \
  --action-on-unmanage deleteResources --deny-settings-mode none --yes
```

Its outputs (`tenantId`, `storageAccount`, `storageContainer`, `writerClientId`, `readerClientId`) are the action's inputs. Read the parameter descriptions before deploying, in particular:

- **`writerSubjects` / `readerSubjects`** are exact-match GitHub OIDC subjects (no wildcards). See [Building the subjects](#building-the-subjects) for their format, which differs between repositories.
- **`manageLifecyclePolicy`** replaces the storage account's *entire* lifecycle policy. Leave it off for a shared existing account that has rules of its own.
- With an existing account in another resource group, the deploying identity also needs rights to create the container and the role assignments there.

### Stacks and what they delete

- **Redeploy with the same stack name** to change the parameters. The stack deletes whatever the template no longer declares: credentials of removed subjects, role assignments of removed groups.
- **That includes storage the stack created:** the stack owns a storage account it created and, unless `createContainer = false`, the container. Switching either off, renaming the container or deleting the stack deletes them, and the cache with them. An existing account (`createStorageAccount = false`) or container (`createContainer = false`) is only referred to, never managed.
- **To hand over something the stack already owns**, e.g. a container that turns out to be shared, redeploy once with `createContainer = false` and `--action-on-unmanage detachAll`. The stack then stops managing it without deleting it. Later deployments can go back to `deleteResources`.
- **Several deployments can share a container**, as long as at most one owns it (the others use `createContainer = false`) and **each has its own identities** (a different `namePrefix`): each stack deletes what isn't in its own parameters.
- **Deployments made before credentials were named after their subjects** have credentials named `github-0`, `github-1`, …. Redeploying them fails, because the renamed credentials repeat the same subjects. Delete the old ones first (`az identity federated-credential delete -g <rg> --identity-name <prefix>-ci-<writer|reader> -n github-<n> --yes`), or deploy with a new `namePrefix`.

### Security model

The server and its config run on the caller's machine, so they are **not** the security boundary. Azure is:

- **Who can write** is decided by the identities' roles on the container and by which GitHub subjects can obtain the writer identity (its federated credentials). Pull requests only ever get the reader, so they can't write at all, not even through a server configured as writable: storage refuses, the server answers `403`, and Nx skips the upload.
- **No-overwrite (`409`) applies to everything that goes through the server.** But a job holding the writer identity also holds a storage credential that can overwrite or delete blobs directly. If such a job is compromised (a malicious dependency in a default-branch build, say), it could replace existing entries. Keep the writer subjects to trusted branches and the writer groups small.

**Possible mitigation: immutable (WORM) storage.** A container-level time-based retention policy makes Azure itself refuse overwrites and deletes for the retention period, whoever asks, so the writer identity can't replace entries either. Tested against this server on a real account: uploads (including large, block-staged ones) still succeed, a repeated upload still gets `409`, and direct overwrites and deletes fail with `BlobImmutableDueToPolicy`. Not enabled here, because:

- **Entries would no longer expire.** Azure's lifecycle management [can't delete any blob in an immutable container](https://learn.microsoft.com/en-us/azure/storage/blobs/lifecycle-management-overview), so something else would have to delete entries once their retention ends (allowed then; overwriting stays forbidden).
- **Only container-level WORM helps.** With version-level WORM an overwrite creates a new current version, which is what the server serves.
- **A locked policy can't be removed or shortened**, and blocks deleting the container while it holds protected blobs. An unlocked policy already stops the CI identities (removing it needs management rights they don't have) and stays reversible.

See [immutable storage for blobs](https://learn.microsoft.com/en-us/azure/storage/blobs/immutable-storage-overview).

## Using it from GitHub Actions

Use the [nx-remote-cache action](https://github.com/graycoreio/github-actions/tree/main/nx-remote-cache) from graycoreio/github-actions. It downloads the server release, starts it in the job with the writer or reader identity, and points Nx at it:

```yaml
permissions:
  id-token: write
  contents: read

steps:
  - uses: graycoreio/github-actions/nx-remote-cache@main
    with:
      tenant-id: <tenantId output>
      writer-client-id: <writerClientId output>
      reader-client-id: <readerClientId output>
      storage-account: <storageAccount output>
      storage-container: <storageContainer output>
  - run: npx nx affected -t build test lint
```

The action's README covers its inputs, fork PRs (no OIDC token, so no remote cache), error messages, and using `continue-on-error` for jobs that can't get a token.

## Developer access

Developers run their own instance, talking to the shared storage as themselves (`az login`), never as CI. What they can do is their own role on the container, enforced by Azure Storage:
- **Storage Blob Data Contributor:** read and write.
- **Storage Blob Data Reader:** read only, with the same setup: storage refuses their uploads, the server answers `403`, and Nx skips them.

`local.bicep` grants these through `readerGroupObjectIds` / `writerGroupObjectIds`, and also gives those groups Storage Blob Delegator on the account. That role only lets them sign user-delegation SAS tokens (`scripts/dev-sas.sh`); a SAS never allows more than the signer's own role on the container.

> [!WARNING]
> Entries a developer writes are served to every CI run with the same task hash. A laptop's builds are less reproducible than CI's (local state, uncommitted changes, tool versions), and a compromised laptop could poison the cache. Keep the writer groups small. Every blob records the static token's name (`dev-<username>` with Docker, whatever you configure for the binary), so a bad entry can be traced.

### Local instance (binary or Docker)

Two ways to run it:

- **Docker:** [`docker-compose.dev.yml`](docker-compose.dev.yml) runs the server next to Microsoft's Azure CLI image. A small sidecar ([`scripts/az-token-endpoint.py`](scripts/az-token-endpoint.py)) stands in for Azure's managed-identity token endpoint, backed by an `az login` kept in a Docker volume. The server image needs no Azure CLI and no keys.

  ```sh
  scripts/dev-compose.sh run --rm az az login --use-device-code   # once
  export NX_CACHE_STORAGE_ACCOUNT=<storageAccount> NX_CACHE_STORAGE_CONTAINER=<storageContainer>
  scripts/dev-compose.sh                                          # docker compose ... up -d
  export NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://127.0.0.1:3000
  export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=local-dev-token
  ```

  - **The sidecar** isn't published to the host, requires a shared secret (`IDENTITY_HEADER`), and only issues Azure Storage tokens.
  - **`scripts/dev-compose.sh`** wraps `docker compose -f docker-compose.dev.yml`. On first use it writes a random secret to `.env`, because Compose can't generate one itself.
  - **The server** is published on `127.0.0.1` only, which is why the well-known `local-dev-token` is acceptable; set `NX_CACHE_DEV_TOKEN_SHA256` to use your own. Entries you write record `dev-<your username>`.
- **Binary:** a [release binary](#install) (or `cargo run --release`) with [`config/developer.example.toml`](config/developer.example.toml) and the Azure CLI installed on your machine (`developer_tools` mode).

The token endpoint also works outside Docker: run `IDENTITY_HEADER=<secret> python3 scripts/az-token-endpoint.py`. Then start any server in `managed_identity` mode with `IDENTITY_ENDPOINT=http://127.0.0.1:8080/metadata/identity/oauth2/token` and the same `IDENTITY_HEADER`.

## Migrating from `@nx/azure-cache` (or another `@nx/*-cache` package)

1. **Use Nx's built-in client.** It exists from Nx 20.8 onwards, and this server is tested with the versions in [Tested Nx versions](#tested-nx-versions).
2. **Uninstall the old cache package, and the `@nx/powerpack-*-cache` equivalents.** Nx looks for those packages *before* it reads `NX_SELF_HOSTED_REMOTE_CACHE_SERVER`. If one is installed and finds its key (`NX_KEY`), Nx uses it and silently ignores this server. Also remove its config from `nx.json` (e.g. the `azure` block); `NX_KEY` / `nx register` are no longer needed. Nx Cloud (`nxCloudId` / `nxCloudAccessToken` in `nx.json`) likewise takes precedence over this server.
3. **Start from an empty container.** Entries written by `@nx/azure-cache` use a different archive layout from Nx's built-in client, which rejects them ("Invalid exit code in cache artifact"), and a rejected read fails the run. They were also writable by every job holding the shared credential, so they should not be trusted. Expect one round of cold builds; the first default-branch push repopulates the cache.
4. **Deploy** `local.bicep` (see [Deploying to Azure](#deploying-to-azure)), with subjects for every workflow trigger that will run the action (next section).
5. **Update every workflow that touched the cache:**
   - Replace the old setup (connection-string env vars, cache-mode variables) with the [nx-remote-cache action](https://github.com/graycoreio/github-actions/tree/main/nx-remote-cache).
   - Grant `id-token: write` to each job that runs it. In a reusable workflow (`workflow_call`), the token is minted in the *caller's* context: if the caller declares `permissions`, it has to grant `id-token: write` too.
   - Put the deployment outputs (tenant id, client ids, storage account and container) in repository variables. None of them are secrets.
6. **Clean up after the switch:** delete the old secrets (connection strings, `NX_KEY`) and the old container. If nothing else uses the account's keys, rotate them and disable shared-key access.

### Building the subjects

GitHub puts the trigger into the OIDC token's `sub`, and Azure matches it exactly against the identities' federated credentials, with no wildcards. First get the repository's subject prefix:

```sh
gh api repos/OWNER/REPO/actions/oidc/customization/sub --jq .sub_claim_prefix
```

Older repositories use the classic `repo:OWNER/REPO`. Repositories created after 2026-07-15, or opted in, use the immutable `repo:OWNER@OWNER_ID/REPO@REPO_ID`. With that prefix:

| Job triggered by | `sub` | Usual list |
|---|---|---|
| push to branch `B` | `<prefix>:ref:refs/heads/B` | writer for the default branch; reader for other branches you want cached |
| pull request (including reusable workflows it calls) | `<prefix>:pull_request` | reader |
| `workflow_dispatch` / `schedule` on branch `B` | `<prefix>:ref:refs/heads/B` | same as a push to `B` |
| tag push or `release` | `<prefix>:ref:refs/tags/<tag>`, different for every tag | can't be listed; see below |
| any job that references an `environment` | `<prefix>:environment:<name>` | reader or writer, per environment |

A trigger with no matching subject can't get a token. For those jobs (tags and releases in particular), choose one of:
- skip the action in that job, so it runs without a remote cache;
- add `continue-on-error: true` to the action step, which does the same automatically.

The `access` input picks between the two identities (`auto`: writer only for a push to the default branch). Whatever it asks for, Azure only issues the writer identity's token to subjects in `writerSubjects`.

## Layout

```
src/
  auth/        Authenticator trait + AuthChain; static_token
  store/       CacheStore trait (get / put_if_absent / ready); memory, azure/
  http/        axum routes: /v1/cache/{hash}, /health
  config.rs    TOML + env config
spec/          Nx OpenAPI spec (tests/spec.rs fails if it changes without the server following)
deploy/        Bicep for Azure
```

> [!NOTE]
> This repo is created almost entirely by claude with human review.
