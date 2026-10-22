# nx-remote-cache

A self-hosted [Nx](https://nx.dev) remote cache server, written in Rust and backed by Azure Blob Storage.

It implements Nx's [remote cache OpenAPI spec](https://nx.dev/docs/kb/self-hosted-caching) (vendored in [`spec/`](spec/nx-remote-cache.openapi.json)). Nx 20.8+ ships its own client for that spec, so a workspace only needs two environment variables:

```sh
export NX_SELF_HOSTED_REMOTE_CACHE_SERVER=https://<server>
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=<token>
```

It replaces `@nx/azure-cache` (and the other `@nx/*-cache` packages). Nx deprecated those because of [CVE-2025-36852 (CREEP)](https://nx.dev/docs/reference/deprecated/self-hosted-cache-packages): any job holding the single read/write credential could overwrite cache entries. This server prevents that in two ways:

- **Entries are never overwritten.** Every upload is an atomic create-if-absent; a second upload for the same hash gets `409`.
- **Writing needs a separate, narrow permission.** By default it is only granted to pushes to the default branch, and Microsoft Entra enforces that, not this server.

## Tested Nx versions

The server is verified end to end against real Nx clients:

| Nx | Status | Scenarios run |
|---|---|---|
| 23.3.0 | Tested | All of the below |
| 24.x | Not yet tested | |

Full scenario list: write, then remote hit with byte-identical output; read-only token (upload skipped, run passes); invalid, expired and role-less tokens (run fails); Entra reader/writer identities; anonymous read; the `mint` exchange; a server URL with a trailing slash.

## How it fits together

```
 Nx client ── Bearer token ──▶ nx-remote-cache ── managed identity / SAS / … ──▶ Azure Blob container
                                 │
                                 └─ verifies the token with one of the configured providers
```

**Client → server** auth providers are pluggable. List any of these under `[[auth.providers]]`:

| Provider | Tokens | Use |
|---|---|---|
| `entra` (recommended) | Entra ID access tokens for the cache's app registration. Their `roles` claim maps to `Cache.Read` or `Cache.Write`. | CI (GitHub OIDC → Entra token via the bundled action) and developers (`az login`). The server holds no keys. |
| `static` | Long-lived tokens, configured as SHA-256 hashes. | A shared read-only developer token, or a break-glass writer. |
| `anonymous` | No token; `read` scope only. | Fork PRs of public repositories, which get neither secrets nor OIDC tokens. |
| `github_mint` (`--features mint`) | The server exchanges GitHub OIDC tokens at `POST /v1/token/github` for HS256 tokens it signs itself, applying `repository_id` / `event_name` / `ref` rules. | Running without Entra. Not compiled in by default, because the server then owns signing keys. |

**Server → storage** uses exactly one explicitly configured mode. There is no credential chain guessing what to use.

| Where it runs | `storage.auth.mode` |
|---|---|
| Azure Container Apps / App Service / VM | `managed_identity` (optional `client_id` for a user-assigned identity) |
| `cargo run` on a dev machine | `developer_tools`: your `az login` |
| Docker on a dev machine | `sas`: [`scripts/dev-sas.sh`](scripts/dev-sas.sh) prints a short-lived user-delegation SAS |
| Elsewhere | `client_secret` (a service principal) or `workload_identity` |
| Azurite | `sas` with `endpoint = "http://…:10000/devstoreaccount1"` |

The identity needs **Storage Blob Data Contributor on the container only**. Shared keys and connection strings are deliberately not supported.

### Behaviour worth knowing

- **Any rejected token fails the Nx run.** Nx treats a `401` from the cache as fatal, even after every task succeeded. That includes expired tokens, deliberately: stale CI credentials should be noticed, not turn into silently skipped cache writes. The CI tokens the action fetches (for the writer/reader managed identities) were observed to last about 24 hours, so expiry within a job is unlikely. Developer tokens from `az account get-access-token` last 60–90 minutes; fetch a new one when it expires. A read-only token's `403` on upload is *not* fatal; Nx just skips the upload.
- **Uploads stream.** Large uploads are staged in 8 MiB blocks and committed with `If-None-Match: *`. Each upload uses about 30 MB; each download about 90 MB, regardless of artifact size.
- **Provenance is recorded.** Every blob carries metadata on who wrote it: provider, subject, client id, or the GitHub repository, event, ref and run.
- **Startup fails fast.** The server checks storage access and OpenID discovery at startup and exits with an explanation (missing container, missing role assignment, expired SAS, …) instead of serving cache misses.
- `/health` is liveness (no dependencies); `/ready` re-checks storage.

## Configuration

The server reads one TOML document, assembled from up to three sources (later ones win):

1. **A file:** `config.toml` in the working directory, or `--config <path>`. The default file is optional when another source supplies the config; a path given with `--config` must exist.
2. **Inline TOML** in the `NX_CACHE_CONFIG_TOML` environment variable. Handy where mounting a file is awkward; this is how the Bicep deployment passes its config.
3. **Per-key overrides** `NX_CACHE__<SECTION>__<KEY>`, e.g. `NX_CACHE__SERVER__LISTEN=0.0.0.0:8080` or `NX_CACHE__STORAGE__CONTAINER=nx-cache-v2`. These work for single values. Lists such as `[[auth.providers]]` can only be set in the file or the inline TOML.

Config never contains secrets: SAS tokens, client secrets and mint keys are read from environment variables that the config *names*. Static tokens are stored as SHA-256 hashes.

### Where the config goes

| How you run it | Where the config lives |
|---|---|
| `cargo run` / the binary | `./config.toml` in the working directory, or `--config <path>` |
| Docker image | Mount it at `/etc/nx-remote-cache/config.toml` (the image's working directory), or pass `NX_CACHE_CONFIG_TOML` |
| `docker compose` (this repo) | [`config/local.toml`](config/local.toml), mounted as above |
| Bicep deployment | Generated from the deployment parameters; see [Configuration in a deployment](#configuration-in-a-deployment) |

Examples: [`config/local.toml`](config/local.toml) (Azurite, well-known local tokens) and [`config/azure.example.toml`](config/azure.example.toml) (Container Apps + Entra).

```sh
nx-remote-cache generate-token        # new random static token + the sha256 to put in config
nx-remote-cache hash-token <token>    # sha256 of an existing token
```

### Reference

Unknown keys are rejected at startup, so a typo fails loudly rather than being ignored.

**`[server]`** (optional)

| Key | Default | |
|---|---|---|
| `listen` | `0.0.0.0:3000` | Address and port |
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
| `managed_identity` | `client_id` (omit for system-assigned) | Azure Container Apps / App Service / VMs |
| `workload_identity` | `tenant_id`, `client_id` (both default to `AZURE_TENANT_ID` / `AZURE_CLIENT_ID`) | AKS-style federation via `AZURE_FEDERATED_TOKEN_FILE` |
| `client_secret` | `tenant_id`, `client_id`, `client_secret_env` (default `AZURE_CLIENT_SECRET`) | A service principal |
| `developer_tools` | — | Your `az login` / `azd auth login` |
| `sas` | `sas_token_env` (default `AZURE_STORAGE_SAS_TOKEN`) | The only mode that works over plain `http` (Azurite) |

**`[[auth.providers]]`**: one table per provider, tried in order. At least one is required.

| `type` | Keys |
|---|---|
| `entra` | `tenant_id`; `audiences` (the API app registration's client id); `read_role` (default `Cache.Read`); `write_role` (default `Cache.Write`); `authority` (default `https://login.microsoftonline.com`) |
| `static` | `tokens = [{ name, sha256, scope }]`, where `scope` is `read` or `read_write` |
| `anonymous` | `scope = "read"` (the only scope allowed) |
| `github_mint` (`--features mint`) | `github_audience`; `signing_keys = [{ kid, secret_env }]` (first signs, all verify; secrets at least 32 bytes); `rules = [{ repository_id, event_name?, ref?, scope }]` (first match wins); `ttl` (default `2h`); `issuer` (default `nx-remote-cache`) |

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
scripts/azurite-test.sh        # everything, including the Azure store against Azurite and the mint feature
```

[`.github/workflows/e2e.yml`](.github/workflows/e2e.yml) runs the whole flow against a deployed server from GitHub (writer on `main`, reader on PRs). It doubles as a worked example of a consumer workflow; it needs the `NX_CACHE_E2E_*` repository variables listed in its header.

## Deploying to Azure

[`deploy/main.bicep`](deploy/main.bicep) creates:

- the `nx-cache` app registration, with v2 tokens and the `Cache.Read` / `Cache.Write` roles;
- two CI managed identities (writer, reader) with GitHub federated credentials, each holding one role;
- storage: either a **new** account (`createStorageAccount = true`: no shared keys, no public access, TLS 1.2 or later), or an **existing** one, in the same or another resource group (`storageResourceGroup`). Either way it adds the cache container and an optional lifecycle (expiry) rule;
- a server identity with Storage Blob Data Contributor on the container;
- the Container App: scale-to-zero, health/readiness probes, and config passed inline.

```sh
az deployment group create -g <resource group> -f deploy/main.bicep -p deploy/example.bicepparam
```

Read the parameter descriptions before deploying, in particular:

- **`writerSubjects` / `readerSubjects`** are exact-match GitHub OIDC subjects (no wildcards). See [Building the subjects](#building-the-subjects) for their format, which differs between repositories.
- **`namePrefix`** also names the Entra app registration, which must be unique in the tenant. Give each deployment its own prefix.
- **`manageLifecyclePolicy`** replaces the storage account's *entire* lifecycle policy. Leave it off for a shared existing account that has rules of its own.
- With an existing account in another resource group, the deploying identity also needs rights to create the container and the role assignment there.
- The deploying identity needs Entra rights to create app registrations and grant app roles.
- The image must be pullable by Container Apps. Make the GHCR package public, or add registry credentials.

### Configuration in a deployment

There is no config file to prepare. The template builds the TOML from its parameters (storage account and container, the server identity, the Entra app, `devReadTokenSha256`, `allowAnonymousRead`) and stores it on the Container App as the `NX_CACHE_CONFIG_TOML` environment variable.

- **To change it, change the parameters and redeploy.** Each change creates a new revision of the app, which restarts it.
- **For settings without a parameter** (e.g. `max_artifact_bytes`, `download_parallelism`, `prefix`), add per-key overrides with the `extraEnv` parameter: `[{ name: 'NX_CACHE__SERVER__MAX_ARTIFACT_BYTES', value: '4294967296' }]`.
- **You can inspect or hand-edit the variables** in the portal (Container App → Containers → Environment variables) or with `az containerapp update --set-env-vars`, e.g. while debugging. The next deployment replaces the container's whole environment and discards hand edits.

## Using it from GitHub Actions

```yaml
permissions:
  id-token: write
  contents: read

steps:
  - uses: graycoreio/nx-remote-cache@v0.1.0
    with:
      server-url: https://<server>
      tenant-id: <tenantId output>
      writer-client-id: <writerClientId output>
      reader-client-id: <readerClientId output>
      api-scope: <apiScope output>
      anonymous-read: true   # only if the server enables the `anonymous` provider
  - run: npx nx affected -t build test lint
```

The action exchanges the job's GitHub OIDC token for an Entra token and exports both Nx variables.

- With `access: auto` it asks for the writer identity only on a push to the default branch. Entra refuses the writer identity to any other subject, whatever the workflow asks for.
- Fork PRs have no OIDC token. With `anonymous-read: true` (for servers with the `anonymous` provider) they read the cache without a token; otherwise the action leaves the remote cache off for them. Pointing Nx at the server without a valid token would fail the run, because Nx treats a `401` from the cache as fatal.
- For the `mint` provider, use `provider: mint`.
- If the token exchange fails, the step fails with the service's own error message. For Entra this includes the OIDC subject GitHub presented, which is what to compare against the federated credentials.
- The action exports the server URL only once it holds a token. So `continue-on-error: true` on the action step turns "no token" into "no remote cache for this job" instead of a failed job.

Developers can read with their own identity, once they are in a group listed in `readerGroupObjectIds`. Because the app requires role assignment, anyone else gets `AADSTS50105`:

```sh
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=$(az account get-access-token --scope <devScope output> --query accessToken -o tsv)
```

## Migrating from `@nx/azure-cache` (or another `@nx/*-cache` package)

1. **Use Nx's built-in client.** It exists from Nx 20.8 onwards, and this server is tested with the versions in [Tested Nx versions](#tested-nx-versions).
2. **Uninstall the old cache package, and the `@nx/powerpack-*-cache` equivalents.** Nx looks for those packages *before* it reads `NX_SELF_HOSTED_REMOTE_CACHE_SERVER`. If one is installed and finds its key (`NX_KEY`), Nx uses it and silently ignores this server. Also remove its config from `nx.json` (e.g. the `azure` block); `NX_KEY` / `nx register` are no longer needed. Nx Cloud (`nxCloudId` / `nxCloudAccessToken` in `nx.json`) likewise takes precedence over this server.
3. **Start from an empty container.** Entries written by `@nx/azure-cache` use a different archive layout from Nx's built-in client, which rejects them ("Invalid exit code in cache artifact"), and a rejected read fails the run. They were also writable by every job holding the shared credential, so they should not be trusted. Expect one round of cold builds; the first default-branch push repopulates the cache.
4. **Deploy** (see [Deploying to Azure](#deploying-to-azure)), with subjects for every workflow trigger that will run the action (next section).
5. **Update every workflow that touched the cache:**
   - Replace the old setup (connection-string env vars, cache-mode variables) with this repo's action.
   - Grant `id-token: write` to each job that runs it. In a reusable workflow (`workflow_call`), the token is minted in the *caller's* context: if the caller declares `permissions`, it has to grant `id-token: write` too.
   - Put the deployment outputs (server URL, tenant id, client ids, API scope) in repository variables. None of them are secrets.
6. **Clean up after the switch:** delete the old secrets (connection strings, `NX_KEY`) and the old container. If nothing else uses the account's keys, rotate them and disable shared-key access.

### Building the subjects

GitHub puts the trigger into the OIDC token's `sub`, and Entra matches it exactly, with no wildcards. First get the repository's subject prefix:

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
- add `continue-on-error: true` to the action step, which does the same automatically;
- read anonymously (public repositories with the `anonymous` provider, `anonymous-read: true`) or with a static read-only token.

The `access` input picks between the two identities (`auto`: writer only for a push to the default branch). Whatever it asks for, Entra only issues the writer token to subjects in `writerSubjects`.

## Layout

```
src/
  auth/        Authenticator trait + AuthChain; entra, static_token, anonymous, mint/ (feature)
  store/       CacheStore trait (get / put_if_absent / ready); memory, azure/
  http/        axum routes: /v1/cache/{hash}, /health, /ready, /v1/token/github (mint)
  config.rs    TOML + env config
spec/          Nx OpenAPI spec (tests/spec.rs fails if it changes without the server following)
deploy/        Bicep for Azure
action.yml     GitHub composite action for CI
```

> [!NOTE]
> This repo is created almost entirely by claude with human review.
