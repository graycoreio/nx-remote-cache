// The two CI identities: a writer and a reader user-assigned managed identity, each trusting
// GitHub Actions OIDC tokens for an exact list of subjects. local.bicep gives them their storage
// roles on the cache container.

param namePrefix string
param location string = resourceGroup().location

@description('GitHub OIDC subjects allowed to WRITE (exact match, no wildcards).')
param writerSubjects array

@description('GitHub OIDC subjects allowed to READ (exact match, no wildcards).')
param readerSubjects array

var githubIssuer = 'https://token.actions.githubusercontent.com'

// Named after the subject, not its position in the list: removing or reordering subjects then
// leaves the other credentials alone. (Positional names retargeted them, and the duplicate
// subject that left behind failed the deployment.) Removing a subject from the list does not
// delete its credential by itself; see docs/deploy.md, "Stacks and what they delete".
func credentialName(subject string) string => 'github-${uniqueString(subject)}'

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
  for subject in writerSubjects: {
    parent: writer
    name: credentialName(subject)
    properties: {
      issuer: githubIssuer
      subject: subject
      audiences: ['api://AzureADTokenExchange']
    }
  }
]

@batchSize(1)
resource readerCredentials 'Microsoft.ManagedIdentity/userAssignedIdentities/federatedIdentityCredentials@2023-01-31' = [
  for subject in readerSubjects: {
    parent: reader
    name: credentialName(subject)
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
