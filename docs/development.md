# Development

## Layout

This repository is an [Nx](https://nx.dev) workspace (`npm install` once) and a Cargo workspace.

| Project     | Path                 | What                                                         |
| ----------- | -------------------- | ------------------------------------------------------------ |
| `server`    | `packages/server`    | The `nx-remote-cache` crate, its Dockerfile and Compose stacks |
| `terraform` | `packages/terraform` | The Azure Terraform module                                   |
| `nx-plugin` | `packages/nx-plugin` | The `@graycoreio/nx-remote-cache` Nx plugin (npm)            |

Run any target with `npx nx run <project>:<target>`, e.g. `npx nx run server:test`. `npx nx show project <project>` lists a project's targets.

## Prerequisites

The server needs Rust 1.89 or later (it uses the 2024 edition). Install it with [rustup](https://rustup.rs), as the [Cargo book](https://doc.rust-lang.org/cargo/getting-started/installation.html) describes, rather than your OS package manager, whose `cargo` is often too old:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then open a new shell, or run `. "$HOME/.cargo/env"`, and check that `cargo --version` shows the rustup toolchain. If it still shows an older version, a system `cargo` is ahead of `~/.cargo/bin` on your `PATH`; remove it (for example `sudo apt remove cargo rustc`).

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

### Nx plugin

The Nx plugin in [`packages/nx-plugin`](../packages/nx-plugin) has its own unit tests:

```sh
npx nx run nx-plugin:test
```

To try it in a workspace with a server built from source, install the package from its directory and set `NX_REMOTE_CACHE_BINARY=<path to target/debug/nx-remote-cache>`.

Releases publish the plugin and one package per platform with its binary, staged from the release archives by [`packages/nx-plugin/scripts/stage-packages.mjs`](../packages/nx-plugin/scripts/stage-packages.mjs).
