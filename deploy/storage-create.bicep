// Creates the storage account and/or the cache container, when local.bicep is asked to. Separate
// from storage.bicep, which refers to both as `existing`: in one template, a created resource and
// an `existing` reference with the same name are a duplicate definition, which ARM rejects.

@description('Storage account name (3-24 lowercase letters and digits, globally unique if created).')
@minLength(3)
@maxLength(24)
param accountName string

param createAccount bool

param location string = resourceGroup().location

@description('SKU when creating the account. Cache entries are reproducible, so LRS is enough.')
param sku string = 'Standard_LRS'

param containerName string

param createContainer bool

resource account 'Microsoft.Storage/storageAccounts@2023-05-01' = if (createAccount) {
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

// Named by path rather than through a parent, so it works whether or not the account was created
// above.
resource container 'Microsoft.Storage/storageAccounts/blobServices/containers@2023-05-01' = if (createContainer) {
  name: '${accountName}/default/${containerName}'
  dependsOn: [account]
  properties: {
    publicAccess: 'None'
  }
}
