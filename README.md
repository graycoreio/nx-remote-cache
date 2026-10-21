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

- **Any rejected token fails the Nx run.** Nx treats a `401` from the cache as fatal, even after every task succeeded. That includes expired tokens, deliberately: stale CI credentials should be noticed, not turn into silently skipped cache writes. Entra tokens last 60–90 minutes from when the action fetched them, so a job that runs longer should run the action again before its later `nx` commands. A read-only token's `403` on upload is *not* fatal; Nx just skips the upload.
- **Uploads stream.** Large uploads are staged in 8 MiB blocks and committed with `If-None-Match: *`. Each upload uses about 30 MB; each download about 90 MB, regardless of artifact size.
- **Provenance is recorded.** Every blob carries metadata on who wrote it: provider, subject, client id, or the GitHub repository, event, ref and run.
- **Startup fails fast.** The server checks storage access and OpenID discovery at startup and exits with an explanation (missing container, missing role assignment, expired SAS, …) instead of serving cache misses.
- `/health` is liveness (no dependencies); `/ready` re-checks storage.

## Configuration

Configuration is read from these sources, later ones winning:

1. `config.toml` (or `--config <path>`);
2. the whole TOML inline in `NX_CACHE_CONFIG_TOML`;
3. per-key `NX_CACHE__SECTION__KEY` overrides, e.g. `NX_CACHE__SERVER__LISTEN=0.0.0.0:8080`.

Config never contains secrets: SAS tokens, client secrets and mint keys are read from the environment variables it names.

Examples: [`config/local.toml`](config/local.toml) (Azurite, well-known local tokens) and [`config/azure.example.toml`](config/azure.example.toml) (Container Apps + Entra).

```sh
nx-remote-cache generate-token        # new random static token + the sha256 to put in config
nx-remote-cache hash-token <token>    # sha256 of an existing token
```

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

## Deploying to Azure

[`deploy/main.bicep`](deploy/main.bicep) creates:

- the `nx-cache` app registration, with v2 tokens and the `Cache.Read` / `Cache.Write` roles;
- two CI managed identities (writer, reader) with GitHub federated credentials, each holding one role;
- the blob container and an optional lifecycle (expiry) rule;
- a server identity with Storage Blob Data Contributor on the container;
- the Container App: scale-to-zero, health/readiness probes, and config passed inline.

```sh
az deployment group create -g <resource group> -f deploy/main.bicep -p deploy/example.bicepparam
```

Read the parameter descriptions before deploying, in particular:

- **`writerSubjects` / `readerSubjects`** are exact-match GitHub OIDC subjects (no wildcards). A job that uses a GitHub `environment` gets a different subject.
- **`manageLifecyclePolicy`** replaces the storage account's *entire* lifecycle policy.
- The deploying identity needs Entra rights to create app registrations and grant app roles.
- The image must be pullable by Container Apps. Make the GHCR package public, or add registry credentials.

## Using it from GitHub Actions

```yaml
permissions:
  id-token: write
  contents: read

steps:
  - uses: graycoreio/nx-remote-cache@v1
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

Developers can read with their own identity:

```sh
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=$(az account get-access-token --scope <devScope output> --query accessToken -o tsv)
```

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
