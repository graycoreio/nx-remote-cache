# Deploying to Azure

The Terraform module in [`deploy/terraform`](../deploy/terraform) creates:

- **Storage:** a new account (no shared keys, no public access, TLS 1.2+), the cache container and an optional expiry rule. The account must not exist yet. Put it in another resource group with `storage_resource_group_name`.
- **Two CI managed identities** with GitHub federated credentials. Writer: Storage Blob Data Contributor on the container. Reader: Storage Blob Data Reader.
- **Developer groups** (optional): `reader_group_object_ids` / `writer_group_object_ids` get the same roles, plus Storage Blob Delegator on the account.

Use it from your own Terraform configuration, pinned to a release tag:

```hcl
provider "azurerm" {
  features {}
}

module "nx_remote_cache" {
  source = "github.com/graycoreio/nx-remote-cache//deploy/terraform?ref=<tag>"

  resource_group_name  = "<resource group>"
  storage_account_name = "<account>"
  writer_subjects      = ["<subject>"]
  reader_subjects      = ["<subject>"]
}

output "nx_remote_cache" {
  value = module.nx_remote_cache
}
```

Then run `terraform init` and `terraform apply`. The outputs (`tenant_id`, `storage_account`, `storage_container`, `writer_client_id`, `reader_client_id`) are the action's inputs. None of them are secrets.

Before deploying:

- `writer_subjects` / `reader_subjects` are exact-match OIDC subjects. See [Building the subjects](#building-the-subjects).
- The resource group must already exist.
- If Azure rejects concurrent federated credential writes with `409`, apply with `-parallelism=1`.

## Building the subjects

Azure matches the OIDC token's `sub` exactly, no wildcards. Get the repository's prefix:

```sh
gh api repos/OWNER/REPO/actions/oidc/customization/sub --jq .sub_claim_prefix
```

Older repositories use `repo:OWNER/REPO`. Repositories created after 2026-07-15, or opted in, use `repo:OWNER@OWNER_ID/REPO@REPO_ID`.

| Job triggered by | `sub` | Usual list |
|---|---|---|
| push to branch `B` | `<prefix>:ref:refs/heads/B` | writer for the default branch, reader for others |
| pull request (and reusable workflows it calls) | `<prefix>:pull_request` | reader |
| `workflow_dispatch` / `schedule` on `B` | `<prefix>:ref:refs/heads/B` | same as a push to `B` |
| tag push or `release` | `<prefix>:ref:refs/tags/<tag>` | cannot be listed; see below |
| job with an `environment` | `<prefix>:environment:<name>` | per environment |

A trigger with no matching subject cannot get a token. For those jobs, skip the action or add `continue-on-error: true` to the step.

The action's `access` input picks the identity (`auto`: writer only on a push to the default branch). Azure only issues the writer token to subjects in `writer_subjects`.

## What Terraform deletes

- **Removing a subject or group** from the variables removes its credential or role assignment on the next apply.
- **The module owns the storage account and container.** Renaming either, or running `terraform destroy`, deletes them and the cache with them.
- **`manage_lifecycle_policy`** writes the account's only lifecycle policy document, replacing any rules added outside the module.

## Immutable storage

A time-based retention policy on the container makes Azure refuse to overwrite or delete any blob until its retention period ends, whatever identity asks. The server works with it: uploads succeed, repeated uploads still return `409`, and direct overwrites fail with `BlobImmutableDueToPolicy`.

The module doesn't set this up. To add it:

```sh
az storage container immutability-policy create \
  --resource-group <resource group> --account-name <account> \
  --container-name <container> --period <days>
```

- Lifecycle management [can't delete blobs in an immutable container](https://learn.microsoft.com/en-us/azure/storage/blobs/lifecycle-management-overview), so you'll need to delete expired entries yourself.
- Use a container-level policy. With a version-level policy, an overwrite still becomes the version the server serves.
- Leave the policy unlocked. A locked policy can't be removed or shortened, and an unlocked one already blocks the CI identities.
