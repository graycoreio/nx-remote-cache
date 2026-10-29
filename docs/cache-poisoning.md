# Cache poisoning

This guide is an emergency protocol for what to do if your cache has been poisoned.

## Revoking write access

Removing someone's write access in Azure doesn't stop their writes immediately. The server runs on their machine with their identity and **Azure's token revocation and expiration delay dictates how quickly that token is invalidated**.

- **A role assigned directly to the person:** removing the role assignment takes effect within about 10 minutes. Azure checks the role on every request, so tokens they already hold don't extend this. See [Role assignment changes are not being detected](https://learn.microsoft.com/en-us/azure/role-based-access-control/troubleshooting#symptom---role-assignment-changes-are-not-being-detected).
- **A role assigned to a group they're in**, as the Terraform module does: removing them from the group can take until their current Azure access token expires, typically 60 to 90 minutes. The token records the groups they were in when it was issued. Stopping their local server (`nx @graycore/nx-remote-cache:stop`) doesn't help, because the Azure CLI on their machine holds the same token.

To cut off write access quickly, you should **remove the role assignment that grants it**, rather than (or as well as) the group membership.

### With the Terraform module

The [Terraform module](../packages/terraform) gives developers write access only through the groups in `writer_group_object_ids`, so there are two steps.

First, remove the person from the writer group. The module doesn't manage group membership, so do it wherever you manage the group, for example with the Azure CLI:

```sh
az ad group member remove \
  --group <writer group object id> \
  --member-id "$(az ad user show --id <their user principal name> --query id -o tsv)"
```

On its own, this takes until their current token expires. To cut off writes within about 10 minutes, also remove the group's role assignment. Take the group out of the module:

```hcl
module "nx_remote_cache" {
  # ...
  writer_group_object_ids = []
}
```

```sh
terraform apply
```

The plan should show `module.nx_remote_cache.azurerm_role_assignment.blob_access["writer-group-<group object id>"]` being destroyed. This takes write access away from everyone in the group, not just that person. Once they're out of the group and their token has expired (allow 90 minutes), put the group back and run `terraform apply` again.

## Swapping the container and identities

You should assume that the entire cache is poisoned. It isn't worth searching for poisoned entries unless you're doing after-the-fact root cause analysis of what was written. Anything malicious written before you revoked access is still in the cache, and CI will use it. It's safest to start over with a new container and new CI identities.

Tokens already issued to the old identities stay valid until they expire, and role changes for managed identities can take [several hours](https://learn.microsoft.com/en-us/azure/role-based-access-control/troubleshooting#symptom---role-assignment-changes-are-not-being-detected). Old tokens get nothing from new identities.

Rename the container:

```hcl
module "nx_remote_cache" {
  # ...
  container_name = "nx-cache-v2" # default: nx-cache-v1
}
```

Then apply, replacing both identities:

```sh
terraform apply \
  -replace='module.nx_remote_cache.azurerm_user_assigned_identity.writer' \
  -replace='module.nx_remote_cache.azurerm_user_assigned_identity.reader'
```

This deletes the old container and everything in it, creates the new one, and recreates the identities, their federated credentials and the role assignments. The identities get new client IDs.

Update `nx.json` with the new values from `terraform output`:

```jsonc
"storageContainer": "nx-cache-v2",
"writerClientId": "<writer_client_id>",
"readerClientId": "<reader_client_id>"
```

If you use the [GitHub Action](running-the-server.md#running-it-in-ci), update its `storage-container`, `writer-client-id` and `reader-client-id` inputs instead.

Expect one slow round of builds while the cache refills. Until a branch has the new `nx.json`, its CI runs without the remote cache. Developers pick up the change on their next pull; `nx @graycore/nx-remote-cache:stop` stops their old server sooner.
