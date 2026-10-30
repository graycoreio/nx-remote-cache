# Server configuration

The server reads a single TOML document built from up to three sources. When the same key is set in more than one place, the later source wins.

1. **File:** `config.toml` in the working directory, or the path given with `--config <path>`. The default file can be left out if another source provides the config.
2. **Inline TOML** in the `NX_CACHE_CONFIG_TOML` environment variable. The action passes its config this way.
3. **Per-key overrides** in the form `NX_CACHE__<SECTION>__<KEY>`, for example `NX_CACHE__SERVER__LISTEN=127.0.0.1:8080`. These only work for single values. Lists such as `[[auth.providers]]` have to be set in the file or in inline TOML.

The config never holds secrets. Client secrets are read from environment variables.

There are two example configs in the repo. [`local.toml`](../packages/server/config/local.toml) points at Azurite, and [`developer.example.toml`](../packages/server/config/developer.example.toml) uses `az login` against real storage.

## Reference

### `[server]` (optional)

| Key                  | Default              | Notes                                                                                                                                                                                                |
| -------------------- | -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `listen`             | `127.0.0.1:3000`     | `NX_CACHE_DEFAULT_LISTEN` changes the default but does not override an explicit `listen`. The Docker image sets it to `0.0.0.0:3000`, so publish the port on loopback with `-p 127.0.0.1:3000:3000`. |
| `max_artifact_bytes` | `2147483648` (2 GiB) | Uploads larger than this are rejected with `413`.                                                                                                                                                    |
| `json_logs`          | `false`              | Write log lines as JSON.                                                                                                                                                                             |

The log level is controlled by [`RUST_LOG`](https://docs.rs/env_logger/latest/env_logger/) and defaults to `info`.

### `[storage]`

| Key                    | Default           | Notes                                                                                                              |
| ---------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------------ |
| `type`                 | required          | `azure`, or `memory` for local smoke tests. `memory` keeps nothing after the server stops and takes no other keys. |
| `account`              | none              | Storage account name. Set either `account` or `endpoint`.                                                          |
| `endpoint`             | none              | Full blob service URL, used instead of `account`. For Azurite this is `http://127.0.0.1:10000/devstoreaccount1`.   |
| `container`            | required          | The container must already exist. The server does not create it.                                                   |
| `prefix`               | `v1`              | Virtual directory inside the container.                                                                            |
| `block_size`           | `8388608` (8 MiB) | Uploads larger than this are sent in blocks of this size.                                                          |
| `download_parallelism` | `4`               | Number of concurrent range requests per download.                                                                  |
| `auth`                 | required          | An inline table that sets `mode` and that mode's keys.                                                             |

`auth.mode` picks how the server signs in to Azure. See [Authentication](authentication.md#azure-credentials) for when to use each one.

| `auth.mode`         | Other keys                                                                                                                                |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `developer_tools`   | None. Uses `az login` or `azd auth login`.                                                                                                |
| `managed_identity`  | `client_id`. Leave it out for a system-assigned identity.                                                                                 |
| `workload_identity` | `tenant_id` and `client_id`, which default to `AZURE_TENANT_ID` and `AZURE_CLIENT_ID`. Reads the token from `AZURE_FEDERATED_TOKEN_FILE`. |
| `client_secret`     | `tenant_id` and `client_id`, both required, and `client_secret_env`, the variable holding the secret (default `AZURE_CLIENT_SECRET`).     |

### `[[auth.providers]]`

How Nx authenticates to the server. At least one provider is required. See [Authentication](authentication.md#validation-token).

| `type`   | Keys                                                                                                                                                                                           |
| -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `static` | `tokens = [{ name, sha256, scope }]`. `sha256` is the hash printed by `nx-remote-cache generate-token`. `scope` is `read` or `read_write`. `name` is recorded on every entry the token writes. |

## Example

```toml
[storage]
type = "azure"
account = "acmecache"
container = "nx"
auth = { mode = "developer_tools" }

[[auth.providers]]
type = "static"
tokens = [{ name = "dev-alice", sha256 = "<sha256>", scope = "read_write" }]
```
