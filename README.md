<div align="center">

[![GitHub Release](https://img.shields.io/github/v/release/graycoreio/nx-remote-cache?color=blue)](https://github.com/graycoreio/nx-remote-cache/releases)
[![Container Image](https://img.shields.io/badge/ghcr.io-graycoreio%2Fnx--remote--cache-blue?logo=docker&logoColor=white)](https://github.com/graycoreio/nx-remote-cache/pkgs/container/nx-remote-cache)
[![CI](https://img.shields.io/github/actions/workflow/status/graycoreio/nx-remote-cache/ci.yml?branch=main&label=CI&labelColor=1a1a1a)](https://github.com/graycoreio/nx-remote-cache/actions/workflows/ci.yml)
[![E2E](https://img.shields.io/github/actions/workflow/status/graycoreio/nx-remote-cache/e2e.yml?branch=main&label=E2E&labelColor=1a1a1a)](https://github.com/graycoreio/nx-remote-cache/actions/workflows/e2e.yml)
[![License](https://img.shields.io/github/license/graycoreio/nx-remote-cache)](LICENSE)

</div>

# @graycore/nx-remote-cache

> A self-hosted client-side Nx remote cache implementation on Azure Blob Storage. You can use `@graycore/nx-remote-cache` to replace `@nx/azure-cache`. See the [migration guide](docs/migrating.md).

## Features

- [No hosted server to operate](docs/how-it-works.md)
- [CREEP-resistant by default](docs/security-model.md#who-can-write)
- [No stored secrets](docs/authentication.md)
- [Write-once entries](docs/security-model.md#immutability)
- [Drop-in replacement for `@nx/azure-cache`](docs/migrating.md)
- [Infrastructure as code](docs/deploy.md)
- [Fails fast](docs/how-it-works.md#behaviour)
- [Upload attribution](docs/authentication.md#upload-attribution)

## Getting Started

First, [deploy](docs/deploy.md) the storage container and the CI identities. We have a pre-built [Terraform module](packages/terraform) that you can use.

Then add the [`@graycore/nx-remote-cache`](packages/nx-plugin) plugin to your workspace. It bundles the server and starts it whenever Nx runs tasks:

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
To run the server yourself instead, with the binary or the GitHub Action, see [Running the server](docs/running-the-server.md).

## CREEP

Nx deprecated the `@nx/azure-cache` package after [CVE-2025-36852 (CREEP)](https://nx.dev/docs/reference/deprecated/self-hosted-cache-packages). As we understand it, a few things factored into that decision:

1. The docs didn't cover separating read and write access, so a typical setup could give pull requests write access, letting them poison the cache for the default branch.
2. "Optimal" caching requires scoping the cache per pull request. On Azure, that means dynamically creating containers and scopes, which is hard to do safely when the client can't be trusted.
3. Maintaining separate self-hosted servers for each major cloud provider is a lot of work, and [Nx Cloud](https://nx.dev/nx-cloud) already solves the problem.

For further information on how we overcome CREEP, please see the [security guide](docs/security-model.md#creep-cve-2025-36852).

## Trade-offs
This project deliberately trades "optimal" caching for "good enough" caching at a much lower cost. 

The performance target of this project is shared build caches from your project's default branch. Instead of scoping the cache per pull request, pull requests can only read from it. Builds on pull requests reuse everything the default branch has already built, but nothing they build themselves is cached, so re-running a pull request, or a second pull request with the same changes, builds those tasks again. 

In exchange, you do not have to operate your own remote cache server and you do not need a Nx Cloud subscription or `NX_KEY`: you pay only for Azure Blob Storage. If this trade-off isn't worth it for you, then just use [Nx Cloud](https://nx.dev/nx-cloud) or [build your own remote server meeting Nx's spec](https://nx.dev/docs/kb/self-hosted-caching).

## Helpful Links

- [Plugin Configuration](packages/nx-plugin/README.md#setup)
- [GitHub Action Configuration](packages/github-action/README.md#inputs)
- [Server Configuration](docs/server-configuration.md)
- [How it works](docs/how-it-works.md)
- [Authentication](docs/authentication.md)
- [Deploying to Azure](docs/deploy.md)
- [Running the server](docs/running-the-server.md)
- [Security model](docs/security-model.md)
- [Cache poisoning](docs/cache-poisoning.md)
- [Development](docs/development.md)

## License

[MIT](LICENSE).
