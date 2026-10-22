// The two CI identities: a writer and a reader user-assigned managed identity, each trusting
// GitHub Actions OIDC tokens for an exact list of subjects. Shared by main.bicep (hosted
// server: the identities get app roles on the cache API) and ci.bicep (server inside the CI
// job: the identities get storage roles directly).

param namePrefix string
param location string = resourceGroup().location

@description('GitHub OIDC subjects allowed to WRITE (exact match, no wildcards).')
param writerSubjects array

@description('GitHub OIDC subjects allowed to READ (exact match, no wildcards).')
param readerSubjects array

var githubIssuer = 'https://token.actions.githubusercontent.com'

resource writer 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' = {
  name: '${namePrefix}-ci-writer'
  location: location
}

resource reader 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' = {
  name: '${namePrefix}-ci-reader'
  location: location
}

// Federated credentials on one identity must be created one at a time (409 otherwise).
@batchSize(1)
resource writerCredentials 'Microsoft.ManagedIdentity/userAssignedIdentities/federatedIdentityCredentials@2023-01-31' = [
  for (subject, i) in writerSubjects: {
    parent: writer
    name: 'github-${i}'
    properties: {
      issuer: githubIssuer
      subject: subject
      audiences: ['api://AzureADTokenExchange']
    }
  }
]

@batchSize(1)
resource readerCredentials 'Microsoft.ManagedIdentity/userAssignedIdentities/federatedIdentityCredentials@2023-01-31' = [
  for (subject, i) in readerSubjects: {
    parent: reader
    name: 'github-${i}'
    properties: {
      issuer: githubIssuer
      subject: subject
      audiences: ['api://AzureADTokenExchange']
    }
  }
]

output writerPrincipalId string = writer.properties.principalId
output writerClientId string = writer.properties.clientId
output readerPrincipalId string = reader.properties.principalId
output readerClientId string = reader.properties.clientId
