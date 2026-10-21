// nx-remote-cache on Azure Container Apps, with Entra ID as the client token issuer.
//
// Deploy into the resource group that holds the storage account:
//   az deployment group create -g <rg> -f deploy/main.bicep -p deploy/example.bicepparam
//
// The deploying identity needs, besides Owner/RBAC on the resource group, Entra permission to
// create app registrations (e.g. the Application Developer role) and to grant app roles.

extension microsoftGraphV1

@description('Prefix for every resource name.')
param namePrefix string = 'nx-remote-cache'

param location string = resourceGroup().location

@description('Existing storage account (in this resource group) that holds the cache.')
param storageAccountName string

@description('Blob container for cache entries. Created if missing.')
param containerName string = 'nx-cache-v1'

@description('Cache entries are deleted this many days after creation.')
param retentionDays int = 30

@description('''Write the account's lifecycle management policy. WARNING: an account has a single
policy document, so this REPLACES any existing lifecycle rules on the account.''')
param manageLifecyclePolicy bool = false

@description('Container image to run, e.g. ghcr.io/graycoreio/nx-remote-cache:0.1.0')
param image string

@description('''GitHub OIDC subjects allowed to WRITE (exact match, no wildcards), e.g.
repo:graycoreio/daffodil:ref:refs/heads/develop''')
param writerSubjects array

@description('GitHub OIDC subjects allowed to READ, e.g. repo:graycoreio/daffodil:pull_request')
param readerSubjects array

@description('Entra group object ids whose members may read the cache with their own az login.')
param readerGroupObjectIds array = []

@description('Allow requests without a token to read (e.g. fork PRs of a public repository).')
param allowAnonymousRead bool = false

@description('Optional sha256 of a static read-only token (from `nx-remote-cache generate-token`).')
param devReadTokenSha256 string = ''

var tenantId = tenant().tenantId
var apiUniqueName = namePrefix
var apiIdentifierUri = 'api://${tenantId}/${apiUniqueName}'
var readRoleId = guid(resourceGroup().id, namePrefix, 'Cache.Read')
var writeRoleId = guid(resourceGroup().id, namePrefix, 'Cache.Write')
var userScopeId = guid(resourceGroup().id, namePrefix, 'user_impersonation')
var azureCliAppId = '04b07795-8ddb-461a-bbee-02f9e1bf7b46'
var githubIssuer = 'https://token.actions.githubusercontent.com'
var storageBlobDataContributor = 'ba92f5b4-2d11-453d-a403-e96b0029c9fe'

// ---------------------------------------------------------------- Entra: the cache API

resource api 'Microsoft.Graph/applications@v1.0' = {
  uniqueName: apiUniqueName
  displayName: 'Nx remote cache (${namePrefix})'
  signInAudience: 'AzureADMyOrg'
  identifierUris: [apiIdentifierUri]
  api: {
    // v2 tokens: `iss` = https://login.microsoftonline.com/<tenant>/v2.0, `aud` = this appId.
    requestedAccessTokenVersion: 2
    oauth2PermissionScopes: [
      {
        id: userScopeId
        value: 'user_impersonation'
        type: 'User'
        isEnabled: true
        adminConsentDisplayName: 'Access the Nx remote cache'
        adminConsentDescription: 'Lets developers read the Nx remote cache with their own identity.'
        userConsentDisplayName: 'Access the Nx remote cache'
        userConsentDescription: 'Read the Nx remote cache on your behalf.'
      }
    ]
    // Lets developers run `az account get-access-token --scope <api>/user_impersonation`.
    preAuthorizedApplications: [
      {
        appId: azureCliAppId
        delegatedPermissionIds: [userScopeId]
      }
    ]
  }
  appRoles: [
    {
      id: readRoleId
      value: 'Cache.Read'
      displayName: 'Read the Nx remote cache'
      description: 'Download cache entries.'
      allowedMemberTypes: ['Application', 'User']
      isEnabled: true
    }
    {
      id: writeRoleId
      value: 'Cache.Write'
      displayName: 'Write the Nx remote cache'
      description: 'Download and upload cache entries.'
      allowedMemberTypes: ['Application']
      isEnabled: true
    }
  ]
}

resource apiSp 'Microsoft.Graph/servicePrincipals@v1.0' = {
  appId: api.appId
  // Without an assignment, tokens carry no `roles` claim and the server rejects them.
  appRoleAssignmentRequired: true
}

// ---------------------------------------------------------------- CI identities

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

resource writerRole 'Microsoft.Graph/appRoleAssignedTo@v1.0' = {
  appRoleId: writeRoleId
  principalId: writer.properties.principalId
  resourceId: apiSp.id
}

resource readerRole 'Microsoft.Graph/appRoleAssignedTo@v1.0' = {
  appRoleId: readRoleId
  principalId: reader.properties.principalId
  resourceId: apiSp.id
}

resource groupReadRoles 'Microsoft.Graph/appRoleAssignedTo@v1.0' = [
  for groupId in readerGroupObjectIds: {
    appRoleId: readRoleId
    principalId: groupId
    resourceId: apiSp.id
  }
]

// ---------------------------------------------------------------- Storage

resource storage 'Microsoft.Storage/storageAccounts@2023-05-01' existing = {
  name: storageAccountName
}

resource blobService 'Microsoft.Storage/storageAccounts/blobServices@2023-05-01' existing = {
  parent: storage
  name: 'default'
}

resource container 'Microsoft.Storage/storageAccounts/blobServices/containers@2023-05-01' = {
  parent: blobService
  name: containerName
  properties: {
    publicAccess: 'None'
  }
}

resource lifecycle 'Microsoft.Storage/storageAccounts/managementPolicies@2023-05-01' = if (manageLifecyclePolicy) {
  parent: storage
  name: 'default'
  properties: {
    policy: {
      rules: [
        {
          name: '${namePrefix}-expire'
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

// ---------------------------------------------------------------- The server

// User-assigned so the role assignment exists before the app first starts (the server checks
// storage access at startup and exits if it is missing).
resource serverIdentity 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' = {
  name: '${namePrefix}-server'
  location: location
}

resource serverBlobAccess 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  scope: container
  name: guid(container.id, serverIdentity.id, storageBlobDataContributor)
  properties: {
    roleDefinitionId: subscriptionResourceId('Microsoft.Authorization/roleDefinitions', storageBlobDataContributor)
    principalId: serverIdentity.properties.principalId
    principalType: 'ServicePrincipal'
  }
}

resource logs 'Microsoft.OperationalInsights/workspaces@2023-09-01' = {
  name: '${namePrefix}-logs'
  location: location
  properties: {
    sku: { name: 'PerGB2018' }
    retentionInDays: 30
  }
}

resource environment 'Microsoft.App/managedEnvironments@2024-03-01' = {
  name: '${namePrefix}-env'
  location: location
  properties: {
    appLogsConfiguration: {
      destination: 'log-analytics'
      logAnalyticsConfiguration: {
        customerId: logs.properties.customerId
        sharedKey: logs.listKeys().primarySharedKey
      }
    }
  }
}

// The whole server config, passed inline. It contains no secrets.
var configLines = concat(
  [
    '[server]'
    'listen = "0.0.0.0:3000"'
    'json_logs = true'
    '[storage]'
    'type = "azure"'
    'account = "${storageAccountName}"'
    'container = "${containerName}"'
    'auth = { mode = "managed_identity", client_id = "${serverIdentity.properties.clientId}" }'
    '[[auth.providers]]'
    'type = "entra"'
    'tenant_id = "${tenantId}"'
    'audiences = ["${api.appId}"]'
  ],
  empty(devReadTokenSha256)
    ? []
    : [
        '[[auth.providers]]'
        'type = "static"'
        'tokens = [{ name = "dev-readonly", sha256 = "${devReadTokenSha256}", scope = "read" }]'
      ],
  allowAnonymousRead
    ? [
        '[[auth.providers]]'
        'type = "anonymous"'
        'scope = "read"'
      ]
    : []
)

resource app 'Microsoft.App/containerApps@2024-03-01' = {
  name: namePrefix
  location: location
  identity: {
    type: 'UserAssigned'
    userAssignedIdentities: {
      '${serverIdentity.id}': {}
    }
  }
  dependsOn: [serverBlobAccess]
  properties: {
    managedEnvironmentId: environment.id
    configuration: {
      ingress: {
        external: true
        targetPort: 3000
        transport: 'http'
        allowInsecure: false
      }
    }
    template: {
      containers: [
        {
          name: 'nx-remote-cache'
          image: image
          // Each in-flight download buffers up to ~90 MiB (see `download_parallelism`).
          resources: {
            cpu: json('0.5')
            memory: '1Gi'
          }
          env: [
            { name: 'NX_CACHE_CONFIG_TOML', value: join(configLines, '\n') }
          ]
          probes: [
            {
              type: 'Liveness'
              httpGet: { path: '/health', port: 3000 }
            }
            {
              type: 'Readiness'
              httpGet: { path: '/ready', port: 3000 }
              periodSeconds: 30
            }
          ]
        }
      ]
      scale: {
        minReplicas: 0
        maxReplicas: 3
        rules: [
          {
            name: 'concurrent-requests'
            http: { metadata: { concurrentRequests: '8' } }
          }
        ]
      }
    }
  }
}

output url string = 'https://${app.properties.configuration.ingress.fqdn}'
output tenantId string = tenantId
output apiClientId string = api.appId
output apiScope string = '${apiIdentifierUri}/.default'
output devScope string = '${apiIdentifierUri}/user_impersonation'
output writerClientId string = writer.properties.clientId
output readerClientId string = reader.properties.clientId
