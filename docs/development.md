# Development

## Layout

This repository is an [Nx](https://nx.dev) workspace (`npm install` once) and a Cargo workspace.

| Project     | Path                 | What                                                         |
| ----------- | -------------------- | ------------------------------------------------------------ |
| `server`    | `packages/server`    | The `nx-remote-cache` crate, its Dockerfile and Compose stacks |
| `terraform` | `packages/terraform` | The Azure Terraform module                                   |

Run any target with `npx nx run <project>:<target>`, e.g. `npx nx run server:test`. `npx nx show project <project>` lists a project's targets.

## Running locally

Start Azurite and the server with Docker Compose. The server listens on port 3000.

```sh
npx nx run server:compose
```

The Compose stack builds the server with the `azurite` Cargo feature. It adds a `sas` storage auth mode, which only works against Azurite; release builds don't include it.

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

To run everything, including the Azure storage tests against Azurite (built with the `azurite` feature), use:

```sh
npx nx run server:test
```

## Linting

```sh
npx nx run-many -t lint       # rustfmt + clippy, terraform fmt
npx nx run terraform:validate
npx nx run-many -t fmt        # fix formatting
```
