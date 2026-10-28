# Usage

## Github actions

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

The [action's README](https://github.com/graycoreio/github-actions/tree/main/nx-remote-cache) covers its inputs, fork PRs, error messages and `continue-on-error`.

## Developer access

Each developer runs their own instance of the server, which signs in to Azure with their `az login`. Their role on the container decides whether they can write. See [Developer writes](security.md#developer-writes) for which roles to grant.

### Docker

[`docker-compose.dev.yml`](../packages/server/docker-compose.dev.yml) runs the server alongside a container with the Azure CLI. You sign in once inside that container, and the login is kept in a Docker volume. A small sidecar, [`az-token-endpoint.py`](../packages/server/scripts/az-token-endpoint.py), hands Azure tokens from that login to the server, so the server image doesn't need the Azure CLI or any keys.

Sign in once:

```sh
packages/server/scripts/dev-compose.sh run --rm az az login --use-device-code
```

Start the server:

```sh
export NX_CACHE_STORAGE_ACCOUNT=<storageAccount>
export NX_CACHE_STORAGE_CONTAINER=<storageContainer>
packages/server/scripts/dev-compose.sh
```

Point Nx at it:

```sh
export NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://127.0.0.1:3000
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=local-dev-token
```

`packages/server/scripts/dev-compose.sh` runs `docker compose -f docker-compose.dev.yml up -d` in `packages/server`. The first time, it also writes a random secret to `packages/server/.env` that the server uses to authenticate to the sidecar. The sidecar isn't exposed to your machine and only issues tokens for Azure Storage.

The server is only reachable on `127.0.0.1`, so the shared `local-dev-token` is safe to use. To use your own token, set `NX_CACHE_DEV_TOKEN_SHA256`. Entries you write are tagged with `dev-<your username>`.

### Devcontainer

The docker image can be run as a sidecar service to your devcontainer workspace. It relies on the `azure-cli` service for identity management.

`workspace.docker-compose.yml`:
```yml
x-identity-header: &identity-header ${NX_CACHE_IDENTITY_HEADER:-local-dev-identity-header}
workspace:
  ...

az:
  image: mcr.microsoft.com/azure-cli:2.91.0
  command: ["python3", "/token-endpoint.py"]
  environment:
    IDENTITY_HEADER: *identity-header
    ALLOWED_RESOURCES: https://storage.azure.com
    PORT: "8080"
  volumes:
    - az-login:/root/.azure
    - ./az-token-endpoint.py:/token-endpoint.py:ro # download from https://github.com/graycoreio/nx-remote-cache/blob/main/packages/server/scripts/az-token-endpoint.py
  healthcheck:
    test: ["CMD", "python3", "-c", "import urllib.request; urllib.request.urlopen('http://127.0.0.1:8080/healthz')"]
    interval: 2s
    retries: 15

nx-cache:
  image: ghcr.io/graycoreio/nx-remote-cache:latest
  depends_on:
    az:
      condition: service_healthy
  ports:
    - "127.0.0.1:3333:3000"
  environment:
    IDENTITY_ENDPOINT: http://az:8080/metadata/identity/oauth2/token
    IDENTITY_HEADER: *identity-header
    NX_CACHE_CONFIG_TOML: |
      [server]
      listen = "0.0.0.0:3000"
      [storage]
      type = "azure"
      account = "${NX_CACHE_STORAGE_ACCOUNT:?set NX_CACHE_STORAGE_ACCOUNT}"
      container = "${NX_CACHE_STORAGE_CONTAINER:?set NX_CACHE_STORAGE_CONTAINER}"
      auth = { mode = "managed_identity" }
      [[auth.providers]]
      type = "static"
      tokens = [{ name = "dev-${USER:-unknown}", sha256 = "${NX_CACHE_DEV_TOKEN_SHA256:-0cd48eed49739dfdd99efaef37d251aae5a80866e5954763c6597943fff5ce9b}", scope = "read_write" }]
```
In a `.env` file sibling to `workspace.docker-compose.yml`:
```
NX_CACHE_IDENTITY_HEADER=...
NX_CACHE_DEV_TOKEN_SHA256=...
NX_CACHE_STORAGE_ACCOUNT=...
NX_CACHE_STORAGE_CONTAINER=...
```
And finally in a `.env` in your nx workspace (or somewhere in your env vars inside the devcontainer):
```
NX_SELF_HOSTED_REMOTE_CACHE_SERVER=...
NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=...
```
After rebuilding your devcontainer, get a shell in the `az` container. For example, in the directory that contains `workspace.docker-compose.yml`:
```sh
docker compose -f workspace.docker-compose.yml exec az az login --use-device-code
```

### Binary

You can also run a [release binary](install.md) directly, using your own Azure CLI login.

```sh
cp packages/server/config/developer.example.toml config.toml   # fill in the <values>
az login
nx-remote-cache
```

The comments in [`packages/server/config/developer.example.toml`](../packages/server/config/developer.example.toml) explain each value. Then point Nx at `http://127.0.0.1:3000` with the token you put in the config.
