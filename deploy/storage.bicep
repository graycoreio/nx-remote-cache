// Storage for nx-remote-cache: a new or existing account, the cache container, the server's
// role assignment on that container only, and an optional lifecycle (expiry) rule.
// Deployed by main.bicep as a module, so the account may live in another resource group.

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

@description('Principal (object) id of the server identity, granted Storage Blob Data Contributor.')
param serverPrincipalId string

param manageLifecyclePolicy bool
param retentionDays int
param ruleName string

var storageBlobDataContributor = 'ba92f5b4-2d11-453d-a403-e96b0029c9fe'

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

resource serverBlobAccess 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  scope: container
  name: guid(container.id, serverPrincipalId, storageBlobDataContributor)
  properties: {
    roleDefinitionId: subscriptionResourceId('Microsoft.Authorization/roleDefinitions', storageBlobDataContributor)
    principalId: serverPrincipalId
    principalType: 'ServicePrincipal'
  }
}

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
