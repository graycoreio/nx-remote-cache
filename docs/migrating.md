# Migrating from `@nx/azure-cache`

1. **Upgrade Nx.** You need Nx 20.8 or later, which includes the built-in remote cache client.

2. **Remove `@nx/azure-cache`.** Uninstall the package and delete its config from `nx.json`. Nx checks for the package before it reads `NX_SELF_HOSTED_REMOTE_CACHE_SERVER`, so if it is still installed and finds `NX_KEY`, Nx ignores this server without any warning.

3. **Start with an empty container.** The built-in client cannot read entries written by `@nx/azure-cache` and fails the run with "Invalid exit code in cache artifact". Those entries could also be overwritten by any job, so they should not be trusted. The first builds after the switch will be cold.

4. **Deploy the infrastructure.** Follow [Deploying to Azure](deploy.md), and add a subject for every workflow trigger that will use the cache.

5. **Update your workflows.** Replace the connection string and any cache settings with the [nx-remote-cache action](https://github.com/graycoreio/github-actions/tree/main/nx-remote-cache). Give each job that runs it `id-token: write`. If the job is in a reusable workflow, the calling workflow must grant `id-token: write` as well. Store the deployment outputs in repository variables, since none of them are secrets.

6. **Clean up.** Delete the old connection string secret, `NX_KEY` and the old container. If nothing else uses the storage account keys, rotate them and turn off shared key access.
