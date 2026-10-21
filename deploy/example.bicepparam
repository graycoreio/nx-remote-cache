// Example: the daffodil cache. Subjects use GitHub's classic `sub` format; repositories created
// after 2026-07-15 use the immutable `repo:OWNER@ID/REPO@ID:...` form instead. Check a real
// token (e.g. with github/actions-oidc-debugger) before relying on these.
using 'main.bicep'

param storageAccountName = 'daffodilnx'
param containerName = 'nx-cache-v1'
param image = 'ghcr.io/graycoreio/nx-remote-cache:0.1.0'

// Only pushes to develop may write. A job that references a GitHub `environment` gets an
// environment-based subject instead and will match neither list.
param writerSubjects = [
  'repo:graycoreio/daffodil:ref:refs/heads/develop'
]
param readerSubjects = [
  'repo:graycoreio/daffodil:pull_request'
]

// daffodil is public: fork PRs (which get no OIDC token) may read.
param allowAnonymousRead = true
