// Azure resources for nx-remote-cache. The server runs next to whoever uses it, inside a CI job
// (graycoreio/github-actions/nx-remote-cache) or on a developer's machine, and talks to Blob
// Storage directly with that caller's identity. This template only creates what that needs:
//
// - storage (a new or existing account) and the cache container;
// - two CI managed identities trusting GitHub OIDC subjects: the writer gets Storage Blob Data
//   Contributor on the container, the reader Storage Blob Data Reader;
// - optionally, the same storage roles for developer groups, plus Storage Blob Delegator on the
//   account so they can sign user-delegation SAS tokens (scripts/dev-sas.sh).
//
// Who may write is decided by those role assignments, enforced by Azure: a local instance and
// its config run on the caller's machine, so they are not a security boundary.
//
// Deploy it as a stack, so that subjects and groups removed from the parameters lose their access
// (a plain deployment leaves their credentials and role assignments in place):
//
//   az stack group create -n <stack> -g <rg> -f deploy/local.bicep -p <params> \
//     --action-on-unmanage deleteResources --deny-settings-mode none --yes

@description('Prefix for the identity names.')
param namePrefix string = 'nx-remote-cache'

param location string = resourceGroup().location

@description('Storage account that holds the cache.')
param storageAccountName string

@description('Create the storage account (locked down: no shared keys, no public access). If false, use an existing one.')
param createStorageAccount bool = false

@description('Resource group of the storage account. Defaults to the deployment resource group.')
param storageResourceGroup string = resourceGroup().name

@description('Blob container for cache entries.')
param containerName string = 'nx-cache-v1'

@description('''Create the container. Set to false to use a container another deployment owns (e.g. a
shared one): as a stack, this deployment would otherwise manage it, and deleting the
stack would delete the container and the cache in it.''')
param createContainer bool = true

@description('Cache entries are deleted this many days after creation.')
param retentionDays int = 30

@description('''Write the account's lifecycle management policy. WARNING: an account has a single
policy document, so this REPLACES any existing lifecycle rules on the account.''')
param manageLifecyclePolicy bool = false

@description('''GitHub OIDC subjects allowed to WRITE (exact match, no wildcards), e.g.
repo:graycoreio/daffodil:ref:refs/heads/develop''')
param writerSubjects array

@description('GitHub OIDC subjects allowed to READ, e.g. repo:graycoreio/daffodil:pull_request')
param readerSubjects array

@description('''Entra group object ids whose members may READ the cache from their own machines
(Storage Blob Data Reader on the container), running the server locally with `az login`.''')
param readerGroupObjectIds array = []

@description('''Entra group object ids whose members may also WRITE from their own machines (Storage Blob
Data Contributor). Entries they write are served to CI; keep this group small.''')
param writerGroupObjectIds array = []

module identities 'identities.bicep' = {
  name: '${namePrefix}-identities'
  params: {
    namePrefix: namePrefix
    location: location
    writerSubjects: writerSubjects
    readerSubjects: readerSubjects
  }
}

module storage 'storage.bicep' = {
  name: '${namePrefix}-storage'
  scope: resourceGroup(storageResourceGroup)
  params: {
    accountName: storageAccountName
    createAccount: createStorageAccount
    location: location
    containerName: containerName
    createContainer: createContainer
    access: concat(
      [
        { principalId: identities.outputs.writerPrincipalId, role: 'contributor' }
        { principalId: identities.outputs.readerPrincipalId, role: 'reader' }
      ],
      map(readerGroupObjectIds, id => { principalId: id, role: 'reader', principalType: 'Group' }),
      map(writerGroupObjectIds, id => { principalId: id, role: 'contributor', principalType: 'Group' })
    )
    // Lets developers sign a user-delegation SAS (scripts/dev-sas.sh) for their own role.
    delegatorGroupObjectIds: union(readerGroupObjectIds, writerGroupObjectIds)
    manageLifecyclePolicy: manageLifecyclePolicy
    retentionDays: retentionDays
    ruleName: '${namePrefix}-expire'
  }
}

// Inputs for graycoreio/github-actions/nx-remote-cache.
output tenantId string = tenant().tenantId
output storageAccount string = storageAccountName
output storageContainer string = containerName
output writerClientId string = identities.outputs.writerClientId
output readerClientId string = identities.outputs.readerClientId
