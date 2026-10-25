// Storage for nx-remote-cache: a new or existing account, the cache container, data-plane role
// assignments on that container only, and an optional lifecycle (expiry) rule.
// Deployed as a module (by hosted.bicep and local.bicep), so the account may live in another
// resource group.

@description('Storage account name (3-24 lowercase letters and digits, globally unique if created).')
@minLength(3)
@maxLength(24)
param accountName string

@description('Create the account. If false, an existing account with this name is used.')
param createAccount bool

param location string = resourceGroup().location

@description('SKU when creating the account. Cache entries are reproducible, so LRS is enough.')
param sku string = 'Standard_LRS'

param containerName string

@description('Identities to grant access to the container: `contributor` (read and write) or `reader`.')
param access blobAccess[]

@description('''Entra groups that may create user-delegation SAS tokens (scripts/dev-sas.sh) for the
container. Delegation keys can only be granted at account scope; a SAS signed with one never
allows more than the signer's own role on the container.''')
param delegatorGroupObjectIds array = []

param manageLifecyclePolicy bool
param retentionDays int
param ruleName string

type blobAccess = {
  principalId: string
  role: 'contributor' | 'reader'
  @description('Defaults to ServicePrincipal (managed identities).')
  principalType: ('ServicePrincipal' | 'Group' | 'User')?
}

var roleIds = {
  contributor: 'ba92f5b4-2d11-453d-a403-e96b0029c9fe' // Storage Blob Data Contributor
  reader: '2a2b9908-6ea1-4ae2-8e65-a410df84e7d1' // Storage Blob Data Reader
}

resource created 'Microsoft.Storage/storageAccounts@2023-05-01' = if (createAccount) {
  name: accountName
  location: location
  kind: 'StorageV2'
  sku: { name: sku }
  properties: {
    // The server only ever uses Entra identities or user-delegation SAS: no account keys.
    allowSharedKeyAccess: false
    allowBlobPublicAccess: false
    minimumTlsVersion: 'TLS1_2'
    supportsHttpsTrafficOnly: true
    defaultToOAuthAuthentication: true
    accessTier: 'Hot'
  }
}

// Refers to the account whether it was created above or already existed.
resource account 'Microsoft.Storage/storageAccounts@2023-05-01' existing = {
  name: accountName
}

resource blobService 'Microsoft.Storage/storageAccounts/blobServices@2023-05-01' existing = {
  parent: account
  name: 'default'
}

resource container 'Microsoft.Storage/storageAccounts/blobServices/containers@2023-05-01' = {
  parent: blobService
  name: containerName
  dependsOn: [created]
  properties: {
    publicAccess: 'None'
  }
}

resource blobAccessAssignments 'Microsoft.Authorization/roleAssignments@2022-04-01' = [
  for grant in access: {
    scope: container
    name: guid(container.id, grant.principalId, roleIds[grant.role])
    properties: {
      roleDefinitionId: subscriptionResourceId('Microsoft.Authorization/roleDefinitions', roleIds[grant.role])
      principalId: grant.principalId
      principalType: grant.?principalType ?? 'ServicePrincipal'
    }
  }
]

var delegatorRoleId = 'db58b8e5-c6ad-4a2a-8342-4190687cbf4a' // Storage Blob Delegator

resource delegatorAssignments 'Microsoft.Authorization/roleAssignments@2022-04-01' = [
  for groupId in delegatorGroupObjectIds: {
    scope: account
    name: guid(account.id, groupId, delegatorRoleId)
    dependsOn: [created]
    properties: {
      roleDefinitionId: subscriptionResourceId('Microsoft.Authorization/roleDefinitions', delegatorRoleId)
      principalId: groupId
      principalType: 'Group'
    }
  }
]

// An account has a single lifecycle policy document: writing it replaces any existing rules.
resource lifecycle 'Microsoft.Storage/storageAccounts/managementPolicies@2023-05-01' = if (manageLifecyclePolicy) {
  parent: account
  name: 'default'
  dependsOn: [created]
  properties: {
    policy: {
      rules: [
        {
          name: ruleName
          enabled: true
          type: 'Lifecycle'
          definition: {
            filters: {
              blobTypes: ['blockBlob']
              prefixMatch: ['${containerName}/']
            }
            actions: {
              baseBlob: {
                delete: {
                  daysAfterCreationGreaterThan: retentionDays
                }
              }
            }
          }
        }
      ]
    }
  }
}

output containerId string = container.id
