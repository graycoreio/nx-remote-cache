# Running the server

The [Nx plugin](../packages/nx-plugin) is the recommended way to use nx-remote-cache: it starts the server for you, in CI and locally. This guide is for running the server yourself instead, for example where the plugin can't run the bundled binary.

## Running it locally

To run the server locally, download the binary for your platform from the [releases page](https://github.com/graycoreio/nx-remote-cache/releases), check it against `SHA256SUMS` and put it on your `PATH`:

<!-- x-release-please-start-version -->
```sh
version=0.4.2
target=x86_64-unknown-linux-gnu   # or aarch64-unknown-linux-gnu, universal-apple-darwin
base=https://github.com/graycoreio/nx-remote-cache/releases/download/v$version
curl -fLO "$base/nx-remote-cache-$version-$target.tar.gz"
curl -fLO "$base/SHA256SUMS"
sha256sum --check --ignore-missing SHA256SUMS   # on macOS: shasum -a 256 --check --ignore-missing SHA256SUMS
tar -xzf "nx-remote-cache-$version-$target.tar.gz"
sudo install nx-remote-cache /usr/local/bin/
```
<!-- x-release-please-end -->

Then fill in a config, sign in to Azure and start it:

```sh
cp packages/server/config/developer.example.toml config.toml   # fill in the <values>
az login
nx-remote-cache
```

Your role on the storage container decides whether you can write. See [Developer writes](security-model.md#developer-writes).

Point Nx at it with the token from your config:

```sh
export NX_SELF_HOSTED_REMOTE_CACHE_SERVER=http://127.0.0.1:3000
export NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN=<token>
```

## Running it in CI

To run the server in GitHub Actions, use the [action](../packages/github-action):

<!-- x-release-please-start-version -->
```yaml
permissions:
  id-token: write
  contents: read

steps:
  - uses: graycoreio/nx-remote-cache/packages/github-action@v0.4.2
    with:
      tenant-id: <tenantId output>
      writer-client-id: <writerClientId output>
      reader-client-id: <readerClientId output>
      storage-account: <storageAccount output>
      storage-container: <storageContainer output>
  - run: npx nx affected -t build test lint
```
<!-- x-release-please-end -->

The [action's README](../packages/github-action/README.md) covers its inputs, fork PRs, error messages and `continue-on-error`.
