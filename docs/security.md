# Security model

The server runs on the caller's own machine, so it can't be trusted to enforce anything. Azure enforces access instead.

## CREEP (CVE-2025-36852)

[CVE-2025-36852](https://nx.dev/docs/reference/deprecated/self-hosted-cache-packages), known as CREEP, is the vulnerability that led Nx to deprecate `@nx/azure-cache`. That package took a single credential, so the documented setup gave every CI job the same read/write access, including jobs for pull requests. Teams that understood the risk could give pull requests a separate read-only credential, but nothing in the package required it. In the usual setup, a pull request could compute the hash of a task that the default branch was about to run and upload a poisoned artifact under that hash first. When the default branch ran the task, Nx restored the poisoned artifact from the cache and used it as the build output.

This is the same class of attack that compromised TanStack in May 2026. A workflow run by a fork's pull request wrote a poisoned pnpm store into the GitHub Actions cache, under the cache scope of `main`. The release workflow on `main` later restored that entry and ran the attacker's code, which published malicious versions of 42 `@tanstack/*` packages to npm. The cache was different, but the flaw was the same: an untrusted job could write cache entries that a trusted job would later restore. See the [TanStack postmortem](https://tanstack.com/blog/npm-supply-chain-compromise-postmortem).

This project closes CREEP by disallowing pull requests writes to the cache. This is enforced in Azure via the identity of the actor interacting with the Cache. In Github, this identity is determined by OIDC. Only the GitHub subjects you configure are allowed to write, usually solely pushes to the default branch. In order to keep code building quickly, however, pull requests get the reader identity, and any attempted uploads to Azure are rejected as a result of the identity's RBAC.

> [!TIP]
> Write-once entries do not prevent CREEP on their own. Keeping untrusted jobs away from the writer identity is what prevents it.

## Who can write

Write access depends on two things: the role each identity has on the container, and which GitHub subjects are allowed to use the writer identity. Pull requests only ever get the reader identity. If a pull request runs a server that is configured to allow writes, Azure still refuses the upload. The server returns a `403` and Nx skips the upload.

## Immutability

The server won't overwrite an existing entry. Uploading a hash that's already in the cache returns `409`.

That only applies to uploads through the server. The writer identity has write access to the container, so a job using it can overwrite or delete blobs by calling Azure directly. If one of those jobs is compromised, say by a malicious dependency in a default-branch build, it can replace cache entries. If that trade-off doesn't work for you, look at [Nx Cloud](https://nx.dev/nx-cloud).

Security here is a shared responsibility. This project gives you separate reader and writer identities, but you decide who gets the writer. 

> [!IMPORTANT]
> Only list trusted branches in the writer subjects, and keep the writer groups small.

## Developer writes

Anything a developer writes to the cache is served to CI on every run with the same task hash. Builds on a laptop are less reproducible than builds in CI, and a compromised laptop can poison the cache.

Developers connect to storage with their own Azure identity, so their role on the container decides what they can do:

| Role                          | Access                                                        |
| ----------------------------- | ------------------------------------------------------------- |
| Storage Blob Data Contributor | Read and write                                                |
| Storage Blob Data Reader      | Read only. Uploads are rejected with `403`, and Nx skips them |

The [Terraform module](../packages/terraform) grants these roles to the groups in `reader_group_object_ids` and `writer_group_object_ids`.

> [!WARNING]
> Consider giving no developers write access at all. Developers still get cache hits from CI, but their own builds won't be shared with each other, so local builds are slower. If that trade-off doesn't work for you, look at [Nx Cloud](https://nx.dev/nx-cloud).

If you need to take away someone's write access, or suspect the cache has been poisoned, see [Cache poisoning](cache-poisoning.md).
