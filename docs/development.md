# Development

## Running locally

Start Azurite and the server with Docker Compose. The server listens on port 3000.

```sh
docker compose up --build
```

Then point Nx at it. Use `local-write-token` to read and write, or `local-read-token` to read only.

```sh
export NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://localhost:3000
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=local-write-token
```

To run against a real storage account, set `auth = { mode = "developer_tools" }` in your config, sign in with `az login`, and start the server:

```sh
cargo run -- --config <file>
```

## Tests

### Unit

Run the unit tests, the HTTP API tests and the checks against the Nx OpenAPI spec with:

```sh
cargo test
```

### Integration

To run everything, including the Azure storage tests against Azurite, use:

```sh
scripts/azurite-test.sh
```
