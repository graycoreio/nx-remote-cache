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

| Key (`azure`)          | Default           | Notes                                                                                                            |
| ---------------------- | ----------------- | ---------------------------------------------------------------------------------------------------------------- |
| `account`              | none              | Storage account name. Set either `account` or `endpoint`.                                                        |
| `endpoint`             | none              | Full blob service URL, used instead of `account`. For Azurite this is `http://127.0.0.1:10000/devstoreaccount1`. |
| `container`            | required          | The container must already exist. The server does not create it.                                                 |
| `prefix`               | `v1`              | Virtual directory inside the container.                                                                          |
| `block_size`           | `8388608` (8 MiB) | Uploads larger than this are sent in blocks of this size.                                                        |
| `download_parallelism` | `4`               | Number of concurrent range requests per download.                                                                |
| `auth`                 | required          | An inline table with exactly one auth mode.                                                                      |

| `auth.mode`         | Other keys                                                                    | Use with                                                |
| ------------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------- |
| `managed_identity`  | `client_id` (leave out for a system-assigned identity)                        | A managed identity endpoint, or the dev compose sidecar |
| `workload_identity` | `tenant_id`, `client_id` (default `AZURE_TENANT_ID` / `AZURE_CLIENT_ID`)      | Federated credentials from `AZURE_FEDERATED_TOKEN_FILE` |
| `client_secret`     | `tenant_id`, `client_id`, `client_secret_env` (default `AZURE_CLIENT_SECRET`) | A service principal                                     |
| `developer_tools`   | none                                                                          | `az login` or `azd auth login`                          |

### `[[auth.providers]]`

At least one provider is required.

| `type`   | Keys                                                                                                                                                         |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `static` | `tokens = [{ name, sha256, scope }]`. `scope` is either `read` or `read_write`. The token's `name` is recorded on every cache entry written with that token. |
