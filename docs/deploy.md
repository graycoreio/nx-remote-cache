# Deploying to Azure

You can use the Terraform module in [`deploy/terraform`](../deploy/terraform) to deploy the necessary infrastructure to Azure.

It contains:

- A new storage account.
- A lifecycle rule that deletes cache entries `retention_days` after creation (30 by default). Turn it off with `manage_lifecycle_policy = false`.
- Writer and reader managed identities for CI, with GitHub federated credentials.
- Optionally, role assignments for developer groups. `writer_group_object_ids` and `reader_group_object_ids` get the same roles as the CI identities.

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

| Job triggered by                               | `sub`                          | Usual list                                       |
| ---------------------------------------------- | ------------------------------ | ------------------------------------------------ |
| push to branch `B`                             | `<prefix>:ref:refs/heads/B`    | writer for the default branch, reader for others |
| pull request (and reusable workflows it calls) | `<prefix>:pull_request`        | reader                                           |
| `workflow_dispatch` / `schedule` on `B`        | `<prefix>:ref:refs/heads/B`    | same as a push to `B`                            |
| tag push or `release`                          | `<prefix>:ref:refs/tags/<tag>` | cannot be listed; see below                      |
| job with an `environment`                      | `<prefix>:environment:<name>`  | per environment                                  |

A trigger with no matching subject cannot get a token. For those jobs, skip the action or add `continue-on-error: true` to the step.

The action's `access` input picks the identity (`auto`: writer only on a push to the default branch). Azure only issues the writer token to subjects in `writer_subjects`.

## What Terraform deletes

- Removing a subject or group from the variables removes its credential or role assignment on the next apply.
- The module owns the storage account and container. Renaming either, or running `terraform destroy`, deletes them along with the cache.
- The expiry rule is the account's only lifecycle policy document, so it replaces any rules added outside the module. Set `manage_lifecycle_policy = false` to manage the policy yourself.