<div align="center">

[![GitHub Release](https://img.shields.io/github/v/release/graycoreio/nx-remote-cache?color=blue)](https://github.com/graycoreio/nx-remote-cache/releases)
[![Container Image](https://img.shields.io/badge/ghcr.io-graycoreio%2Fnx--remote--cache-blue?logo=docker&logoColor=white)](https://github.com/graycoreio/nx-remote-cache/pkgs/container/nx-remote-cache)
[![CI](https://img.shields.io/github/actions/workflow/status/graycoreio/nx-remote-cache/ci.yml?branch=main&label=CI&labelColor=1a1a1a)](https://github.com/graycoreio/nx-remote-cache/actions/workflows/ci.yml)
[![E2E](https://img.shields.io/github/actions/workflow/status/graycoreio/nx-remote-cache/e2e.yml?branch=main&label=E2E&labelColor=1a1a1a)](https://github.com/graycoreio/nx-remote-cache/actions/workflows/e2e.yml)
[![License](https://img.shields.io/github/license/graycoreio/nx-remote-cache)](LICENSE)

</div>

# nx-remote-cache

> A self-hosted Nx remote cache on Azure Blob Storage, with nothing to host and no license fees.

- 🏠 Runs next to Nx, in the CI job or on your laptop
- 🔑 Uses the caller's own Azure identity: GitHub OIDC in CI, `az login` locally
- 🔒 Write-once entries, never overwritten
- 🚫 No shared keys, no connection strings
- ⚡ Works with Nx's built-in client, no plugin
- 🦀 One static Rust binary, or a container image

## Nx Version Support

![Nx 23 Supported](https://img.shields.io/badge/Nx-23-brightgreen.svg?labelColor=2f2b2f&logo=nx&logoColor=white&color=464246&longCache=true&style=flat)

> [!NOTE]
> Needs Nx 20.8 or later.

## Purpose

Nx deprecated `@nx/azure-cache` and the other `@nx/*-cache` packages after [CVE-2025-36852 (CREEP)](https://nx.dev/docs/reference/deprecated/self-hosted-cache-packages): every job shared one read/write credential, so a pull request could poison the cache for the default branch.

We use `nx-remote-cache` to replace it. This project is a Rust implementation of Nx's OpenAPI spec specifically built around Azure Blob storage, authenticating via OIDC.

It implements Nx's [remote cache spec](https://nx.dev/docs/kb/self-hosted-caching) and is intended to run either inside each CI job or on each developer's machine, talking to Azure as its caller.

## Getting Started

1. [Deploy](docs/deploy.md) the container and the CI identities with `deploy/local.bicep`
2. Add the [nx-remote-cache action](https://github.com/graycoreio/github-actions/tree/main/nx-remote-cache) to your workflows:

```yaml
permissions:
  id-token: write
  contents: read

steps:
  - uses: graycoreio/github-actions/nx-remote-cache@main
    with:
      tenant-id: <tenantId>
      writer-client-id: <writerClientId>
      reader-client-id: <readerClientId>
      storage-account: <storageAccount>
      storage-container: <storageContainer>
  - run: npx nx affected -t build test lint
```

3. Optionally, give developers a [local instance](docs/developer-access.md)

## Features

- [Write-once cache entries](docs/security.md)
- [Separate reader and writer identities](docs/deploy.md)
- [No secrets in config](docs/configuration.md)
- [Streaming uploads and downloads](docs/how-it-works.md)
- [Fails fast on bad storage access](docs/how-it-works.md)

## Helpful Links

- [Install](docs/install.md)
- [Configuration](docs/configuration.md)
- [How it works](docs/how-it-works.md)
- [Deploying to Azure](docs/deploy.md)
- [Developer access](docs/developer-access.md)
- [Security model](docs/security.md)
- [Development](docs/development.md)

## Upgrading

- [Migrating from `@nx/azure-cache`](docs/migrating.md)

## License

[MIT](LICENSE).
