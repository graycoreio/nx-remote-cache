# Nx Remote Cache

> [!NOTE]
> Most workspaces should use the [Nx plugin](../nx-plugin) instead of this action. Use this action when the plugin isn't an option, or to run a server built from source.

A GitHub Action that runs an [nx-remote-cache](https://github.com/graycoreio/nx-remote-cache) server in your job and configures [Nx](https://nx.dev) to use it.

The action sets these environment variables for later steps:

- `NX_SELF_HOSTED_REMOTE_CACHE_SERVER`
- `NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN`

Supported runners are Linux (x64, arm64) and macOS.

See [Deploying to Azure](../../docs/deploy.md) to set up the storage and identities with the [Terraform module](../terraform).

## Usage

<!-- x-release-please-start-version -->

```yaml
permissions:
  id-token: write
  contents: read

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - uses: graycoreio/nx-remote-cache/packages/github-action@v0.4.2
        with:
          tenant-id: ${{ vars.NX_CACHE_TENANT_ID }}
          writer-client-id: ${{ vars.NX_CACHE_WRITER_CLIENT_ID }}
          reader-client-id: ${{ vars.NX_CACHE_READER_CLIENT_ID }}
          storage-account: ${{ vars.NX_CACHE_STORAGE_ACCOUNT }}
          storage-container: ${{ vars.NX_CACHE_STORAGE_CONTAINER }}
      - run: npx nx affected -t build test lint
```

<!-- x-release-please-end -->

In a reusable workflow, the calling workflow must also grant `id-token: write`.

## Inputs

| Input               | Default                                   | Description                                                                                                                                                                                                                        |
| ------------------- | ----------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `access`            | `auto`                                    | `auto`, `read` or `write`. `auto` uses the writer identity on pushes to the default branch and the reader identity otherwise. [This only selects the identity. Write access is controlled in Azure.](../../docs/security-model.md) |
| `tenant-id`         |                                           | Azure tenant ID.                                                                                                                                                                                                                   |
| `writer-client-id`  |                                           | Client ID of the writer managed identity.                                                                                                                                                                                          |
| `reader-client-id`  |                                           | Client ID of the reader managed identity.                                                                                                                                                                                          |
| `storage-account`   |                                           | Storage account name.                                                                                                                                                                                                              |
| `storage-container` |                                           | Blob container name.                                                                                                                                                                                                               |
| `storage-prefix`    | `v1`                                      | Path prefix for cache entries in the container.                                                                                                                                                                                    |
| `version`           | `0.4.2` <!-- x-release-please-version --> | Server version or npm dist-tag to run. Defaults to the action's release.                                                                                                                                                           |
| `server-binary`     |                                           | Path to a local server binary to use instead of downloading `version`.                                                                                                                                                             |

## Behavior

If the job has no OIDC token, for example on pull requests from forks, the action skips the cache and the job runs without it.

The step fails if the server can't authenticate, can't access storage, or can't be downloaded. The server log is printed to help debug, including any error from Entra ID. Add `continue-on-error: true` to run without the cache instead of failing, for example on tag pushes that don't match a federated credential.

The server runs in the background until the job ends. Its log path is in `NX_REMOTE_CACHE_LOG`. A post step stops the server and prints the log.

## Server releases

The action downloads the server binary from npm (`@graycore/nx-remote-cache-linux-x64-gnu`, `-linux-arm64-gnu` or `-darwin-universal`) and verifies its integrity hash.

> [!TIP]
> npm packages are published a few minutes after a release is tagged, so a new tag can't be used until they're available.
