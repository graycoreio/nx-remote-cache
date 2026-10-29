# Migrating from `@nx/azure-cache`

You need Nx 20.8 or later, which has the built-in self-hosted cache client.

## 1. Remove `@nx/azure-cache`

Uninstall the package and remove its config from `nx.json`:

```sh
npm uninstall @nx/azure-cache
```

If it's still installed and `NX_KEY` is set, Nx uses it and silently ignores this cache.

## 2. Deploy the infrastructure

Follow [Deploying to Azure](deploy.md). Add a subject for every workflow trigger that uses the cache.

The module creates a new storage account, so the cache starts empty and the first builds are cold. Don't copy the old entries over: Nx's client can't read them (it fails with "Invalid exit code in cache artifact"), and any job could have overwritten them.

> [!NOTE]
> You can reuse your existing storage account, but you must use a different container from the one `@nx/azure-cache` wrote to. 

## 3. Add the plugin

Install the plugin and add the deployment outputs to `nx.json`:

```sh
npm install -D @graycore/nx-remote-cache
```

```jsonc
// nx.json
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

## 4. Update your workflows

Remove the connection string and `NX_KEY` from your workflows. Store the tenant ID and the two client IDs from the deployment outputs as repository variables (they aren't secrets), then give the jobs that run Nx an OIDC token and the CI identities:

```yaml
permissions:
  id-token: write
  contents: read
env:
  NX_REMOTE_CACHE_TENANT_ID: ${{ vars.NX_CACHE_TENANT_ID }}
  NX_REMOTE_CACHE_WRITER_CLIENT_ID: ${{ vars.NX_CACHE_WRITER_CLIENT_ID }}
  NX_REMOTE_CACHE_READER_CLIENT_ID: ${{ vars.NX_CACHE_READER_CLIENT_ID }}
```

> [!TIP]
> If a job is in a reusable workflow, the calling workflow must grant `id-token: write` too.

Check the connection, locally after `az login` or in a CI job:

```sh
npx nx @graycore/nx-remote-cache
```

If you can't use the plugin, see [Running the server](running-the-server.md) for the binary and the GitHub Action.

## 5. Clean up

Delete the old connection string secret, `NX_KEY` and the old container. If nothing else uses the old storage account's keys, rotate them and disable shared key access.
