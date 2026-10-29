<div align="center">

[![GitHub Release](https://img.shields.io/github/v/release/graycoreio/nx-remote-cache?color=blue)](https://github.com/graycoreio/nx-remote-cache/releases)
[![Container Image](https://img.shields.io/badge/ghcr.io-graycoreio%2Fnx--remote--cache-blue?logo=docker&logoColor=white)](https://github.com/graycoreio/nx-remote-cache/pkgs/container/nx-remote-cache)
[![CI](https://img.shields.io/github/actions/workflow/status/graycoreio/nx-remote-cache/ci.yml?branch=main&label=CI&labelColor=1a1a1a)](https://github.com/graycoreio/nx-remote-cache/actions/workflows/ci.yml)
[![E2E](https://img.shields.io/github/actions/workflow/status/graycoreio/nx-remote-cache/e2e.yml?branch=main&label=E2E&labelColor=1a1a1a)](https://github.com/graycoreio/nx-remote-cache/actions/workflows/e2e.yml)
[![License](https://img.shields.io/github/license/graycoreio/nx-remote-cache)](LICENSE)

</div>

# @graycore/nx-remote-cache

> A self-hosted Nx remote cache implementation on Azure Blob Storage

This project provides:

- A platform-specific client-side rust server that runs next to Nx and implements their [Self-Hosted Caching spec](https://nx.dev/docs/kb/self-hosted-caching)
- It uses the caller's own Azure identity, changing depending upon context:
  - GitHub OIDC in CI
  - `az login` locally
- Immutable Storage for write-once entries
- No connection strings
- Works with Nx's built-in client
- A Docker-container image that you can if you cannot run the server due to machine permissions

## Purpose

You can use `@graycore/nx-remote-cache` to replace `@nx/azure-cache`.

This project is a Rust implementation of Nx's OpenAPI spec specifically built around [Azure Blob storage](https://azure.microsoft.com/en-us/products/storage/blobs), authenticating via OIDC.

It implements Nx's [remote cache spec](https://nx.dev/docs/kb/self-hosted-caching) and is intended to run either inside each CI job or as a daemon on each developer's machine.


## CREEP

Nx deprecated the `@nx/azure-cache` package after [CVE-2025-36852 (CREEP)](https://nx.dev/docs/reference/deprecated/self-hosted-cache-packages). They did this for a variety of reasons:

1. Their docs guided every job to share one read/write credential, allowing pull requests to poison the cache for the default branch. 
2. To achieve "optimal" caching capabilities, one needs to enable cache scoping on Pull Requests
   1. It is difficult to do this "out-of-the-box" on Azure without substantial headache of operating your own remote caching server.
3. It makes more sense to just use [Nx Cloud](https://nx.dev/nx-cloud) rather than build and maintain bespoke self-hosted servers for all platforms.

This project deliberately trades "optimal" caching for "good enough" caching at a much lower cost. Instead of scoping the cache per pull request, pull requests can only read from it. They reuse everything the default branch has already built, but nothing they build themselves is cached, so re-running a pull request, or a second pull request with the same changes, builds those tasks again. In exchange, there is no shared cache server to operate and no Nx Cloud subscription: you pay only for Azure Blob Storage. If this trade-off isn't worth it for you, then just use [Nx Cloud](https://nx.dev/nx-cloud).

For further information on how we overcome CREEP, please see the [security guide](docs/security.md#creep-cve-2025-36852).

## Getting Started

First, [deploy](docs/deploy.md) the storage container and the CI identities with the [Terraform module](packages/terraform).

Then add the [`@graycore/nx-remote-cache`](packages/nx-plugin) plugin to your workspace. It bundles the server and starts it whenever Nx runs tasks, in CI (GitHub OIDC) and locally (`az login`), so neither needs any other setup:

```sh
npm install -D @graycore/nx-remote-cache
```

```jsonc
// nx.json, with the deployment outputs
{
  "plugins": [
    {
      "plugin": "@graycore/nx-remote-cache",
      "options": {
        "storageAccount": "<storageAccount>",
        "storageContainer": "<storageContainer>"
      }
    }
  ]
}
```

In GitHub Actions, give the jobs that run Nx an [OIDC token](https://docs.github.com/en/actions/concepts/security/openid-connect) and the CI identities.

```yaml
permissions:
  id-token: write
  contents: read
env:
  NX_REMOTE_CACHE_TENANT_ID: ${{ vars.NX_CACHE_TENANT_ID }}
  NX_REMOTE_CACHE_WRITER_CLIENT_ID: ${{ vars.NX_CACHE_WRITER_CLIENT_ID }}
  NX_REMOTE_CACHE_READER_CLIENT_ID: ${{ vars.NX_CACHE_READER_CLIENT_ID }}
```

Every plugin option can also come from the environment; see the [plugin's README](packages/nx-plugin/README.md#setup).

You can check the status of your connection to the cache with:

```bash
npx nx @graycore/nx-remote-cache
```


### Configuring

The [plugin's README](packages/nx-plugin) covers its options and environment variables.


### Running the caching server manually
To run the server yourself instead, with the GitHub Action, the binary or Docker, see [Usage](docs/usage.md).

## Features

- [Write-once cache entries](docs/security.md)
- [Separate reader and writer identities](docs/deploy.md)
- [No secrets in config](docs/server-configuration.md)
- [Streaming uploads and downloads](docs/how-it-works.md)
- [Fails fast on bad storage access](docs/how-it-works.md)

## Helpful Links

- [Configuration](docs/server-configuration.md)
- [How it works](docs/how-it-works.md)
- [Deploying to Azure](docs/deploy.md)
- [Usage](docs/usage.md)
- [Security model](docs/security.md)
- [Cache poisoning](docs/cache-poisoning.md)
- [Development](docs/development.md)

## Upgrading

- [Migrating from `@nx/azure-cache`](docs/migrating.md)

## License

[MIT](LICENSE).
