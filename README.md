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

You can use `nx-remote-cache` to replace `@nx/azure-cache`.

Nx deprecated the `@nx/azure-cache` package after [CVE-2025-36852 (CREEP)](https://nx.dev/docs/reference/deprecated/self-hosted-cache-packages). They did this for a variety of reasons. Chiefly, their docs guided every job to share one read/write credential, potentially allowing pull requests to poison the cache for the default branch. Luckily, Azure has a authentication model that allows you to work around CREEP while still self-hosting your own cache.

This project is a Rust implementation of Nx's OpenAPI spec specifically built around [Azure Blob storage](https://azure.microsoft.com/en-us/products/storage/blobs), authenticating via OIDC.

It implements Nx's [remote cache spec](https://nx.dev/docs/kb/self-hosted-caching) and is intended to run either inside each CI job or as a daemon on each developer's machine.

## Getting Started

First, [deploy](docs/deploy.md) the storage container and the CI identities with the [Terraform module](deploy/terraform).

### CI

Add the [nx-remote-cache action](https://github.com/graycoreio/github-actions/tree/main/nx-remote-cache) to your workflows, using the deployment outputs:

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

### Locally

[Install](docs/install.md) the binary, copy [`config/developer.example.toml`](config/developer.example.toml) to `config.toml` and fill in the values, then:

```sh
az login
nx-remote-cache
```

Point Nx at it with the token from your config:

```sh
export NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://127.0.0.1:3000
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=<token>
```

To run it with Docker instead, see [Usage](docs/usage.md).

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
- [Usage](docs/usage.md)
- [Security model](docs/security.md)
- [Development](docs/development.md)

## Upgrading

- [Migrating from `@nx/azure-cache`](docs/migrating.md)

## License

[MIT](LICENSE).
