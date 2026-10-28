# Configuration

One TOML document, assembled from up to three sources. Later sources win.

1. **File:** `config.toml` in the working directory, or `--config <path>`. The default file is optional if another source supplies the config.
2. **Inline TOML** in `NX_CACHE_CONFIG_TOML`. This is how the action passes its config.
3. **Per-key overrides** `NX_CACHE__<SECTION>__<KEY>`, e.g. `NX_CACHE__SERVER__LISTEN=127.0.0.1:8080`. Single values only; lists such as `[[auth.providers]]` need the file or inline TOML.

Config never contains secrets. Client secrets come from environment variables the config names. Static tokens are stored as SHA-256 hashes.

| How you run it | Where the config lives |
|---|---|
| Binary / `cargo run` | `./config.toml`, or `--config <path>` |
| Docker image | Mount at `/etc/nx-remote-cache/config.toml`, or pass `NX_CACHE_CONFIG_TOML` |
| `docker compose` (this repo) | [`config/local.toml`](../config/local.toml) |
| The action | Generated per job, passed as `NX_CACHE_CONFIG_TOML` |

Examples: [`config/local.toml`](../config/local.toml) (Azurite) and [`config/developer.example.toml`](../config/developer.example.toml) (`az login`, real storage).

```sh
nx-remote-cache generate-token        # new random token + its sha256
nx-remote-cache hash-token <token>    # sha256 of an existing token
```

## Reference

Unknown keys are rejected at startup.

### `[server]` (optional)

| Key | Default | |
|---|---|---|
| `listen` | `127.0.0.1:3000` | `NX_CACHE_DEFAULT_LISTEN` replaces the default without overriding an explicit `listen`. The image sets it to `0.0.0.0:3000`; publish the port on loopback: `-p 127.0.0.1:3000:3000` |
| `max_artifact_bytes` | `2147483648` (2 GiB) | Larger uploads get `413` |
| `json_logs` | `false` | JSON log lines |

Log level follows `RUST_LOG` (default `info`).

### `[storage]`

`type = "azure"`, or `type = "memory"` (not persistent; smoke tests only).

| Key (`azure`) | Default | |
|---|---|---|
| `account` | — | Storage account name. One of `account` / `endpoint` is required |
| `endpoint` | — | Full blob service URL instead of `account`, e.g. Azurite's `http://127.0.0.1:10000/devstoreaccount1` |
| `container` | required | Must exist; the server never creates it |
| `prefix` | `v1` | Virtual directory inside the container |
| `block_size` | `8388608` (8 MiB) | Uploads above this size go up in blocks of this size |
| `download_parallelism` | `4` | Concurrent range requests per download |
| `auth` | required | Exactly one mode, as an inline table |

| `auth.mode` | Other keys | |
|---|---|---|
| `managed_identity` | `client_id` (omit for system-assigned) | A managed-identity endpoint, or the dev compose sidecar |
| `workload_identity` | `tenant_id`, `client_id` (default `AZURE_TENANT_ID` / `AZURE_CLIENT_ID`) | Federation via `AZURE_FEDERATED_TOKEN_FILE` |
| `client_secret` | `tenant_id`, `client_id`, `client_secret_env` (default `AZURE_CLIENT_SECRET`) | A service principal |
| `developer_tools` | — | `az login` / `azd auth login` |

### `[[auth.providers]]`

At least one is required.

| `type` | Keys |
|---|---|
| `static` | `tokens = [{ name, sha256, scope }]`. `scope` is `read` or `read_write`. `name` is recorded on everything the token writes |
