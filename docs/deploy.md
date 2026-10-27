# Deploying to Azure

[`deploy/local.bicep`](../deploy/local.bicep) creates:

- **Storage:** a new account (`createStorageAccount = true`: no shared keys, no public access, TLS 1.2+) or an existing one (`storageResourceGroup` for another resource group). Either way it adds the container (unless `createContainer = false`) and an optional expiry rule.
- **Two CI managed identities** with GitHub federated credentials. Writer: Storage Blob Data Contributor on the container. Reader: Storage Blob Data Reader.
- **Developer groups** (optional): `readerGroupObjectIds` / `writerGroupObjectIds` get the same roles, plus Storage Blob Delegator on the account.

Deploy it as a deployment stack, so removing a subject or group also removes its credential or role assignment:

```sh
az stack group create -n <stack name> -g <resource group> -f deploy/local.bicep \
  -p storageAccountName=<account> writerSubjects='["<subject>"]' readerSubjects='["<subject>"]' \
  --action-on-unmanage deleteResources --deny-settings-mode none --yes
```

The outputs (`tenantId`, `storageAccount`, `storageContainer`, `writerClientId`, `readerClientId`) are the action's inputs. None of them are secrets.

Before deploying:

- `writerSubjects` / `readerSubjects` are exact-match OIDC subjects. See [Building the subjects](#building-the-subjects).
- `manageLifecyclePolicy` replaces the account's entire lifecycle policy. Leave it off on a shared account.
- An existing account in another resource group needs rights to create the container and role assignments there.

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

The action's `access` input picks the identity (`auto`: writer only on a push to the default branch). Azure only issues the writer token to subjects in `writerSubjects`.

## Stacks and what they delete

- **Redeploy with the same stack name** to change parameters. The stack deletes whatever the template no longer declares.
- **That includes storage the stack created.** Switching `createStorageAccount` or `createContainer` off, renaming the container or deleting the stack deletes them, and the cache with them. Existing resources are only referenced.
- **To hand over something the stack owns**, redeploy once with `createContainer = false` and `--action-on-unmanage detachAll`.
- **Several stacks can share a container** if only one owns it and each has its own `namePrefix`.
- **Stacks deployed before credentials were named after their subjects** have `github-0`, `github-1`, … credentials and fail to redeploy. Delete them first (`az identity federated-credential delete -g <rg> --identity-name <prefix>-ci-<writer|reader> -n github-<n> --yes`) or use a new `namePrefix`.

## Immutable storage

A time-based retention policy on the container makes Azure refuse to overwrite or delete any blob until its retention period ends, whatever identity asks. The server works with it: uploads succeed, repeated uploads still return `409`, and direct overwrites fail with `BlobImmutableDueToPolicy`.

`local.bicep` doesn't set this up. To add it:

```sh
az storage container immutability-policy create \
  --resource-group <resource group> --account-name <account> \
  --container-name <container> --period <days>
```

- Lifecycle management [can't delete blobs in an immutable container](https://learn.microsoft.com/en-us/azure/storage/blobs/lifecycle-management-overview), so you'll need to delete expired entries yourself.
- Use a container-level policy. With a version-level policy, an overwrite still becomes the version the server serves.
- Leave the policy unlocked. A locked policy can't be removed or shortened, and an unlocked one already blocks the CI identities.
