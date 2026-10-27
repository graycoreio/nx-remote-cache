# Developer access

Each developer runs their own instance of the server, which signs in to Azure with their `az login`. Their role on the container decides whether they can write. See [Developer writes](security.md#developer-writes) for which roles to grant.

## Docker

[`docker-compose.dev.yml`](../docker-compose.dev.yml) runs the server alongside a container with the Azure CLI. You sign in once inside that container, and the login is kept in a Docker volume. A small sidecar, [`scripts/az-token-endpoint.py`](../scripts/az-token-endpoint.py), hands Azure tokens from that login to the server, so the server image doesn't need the Azure CLI or any keys.

Sign in once:

```sh
scripts/dev-compose.sh run --rm az az login --use-device-code
```

Start the server:

```sh
export NX_CACHE_STORAGE_ACCOUNT=<storageAccount>
export NX_CACHE_STORAGE_CONTAINER=<storageContainer>
scripts/dev-compose.sh
```

Point Nx at it:

```sh
export NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://127.0.0.1:3000
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=local-dev-token
```

`scripts/dev-compose.sh` runs `docker compose -f docker-compose.dev.yml up -d`. The first time, it also writes a random secret to `.env` that the server uses to authenticate to the sidecar. The sidecar isn't exposed to your machine and only issues tokens for Azure Storage.

The server is only reachable on `127.0.0.1`, so the shared `local-dev-token` is safe to use. To use your own token, set `NX_CACHE_DEV_TOKEN_SHA256`. Entries you write are tagged with `dev-<your username>`.

## Binary

You can also run a [release binary](install.md) directly, using your own Azure CLI login.

```sh
cp config/developer.example.toml config.toml   # fill in the <values>
az login
nx-remote-cache
```

The comments in [`config/developer.example.toml`](../config/developer.example.toml) explain each value. Then point Nx at `http://127.0.0.1:3000` with the token you put in the config.
