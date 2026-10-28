data "azurerm_client_config" "current" {}

data "azurerm_resource_group" "this" {
  name = var.resource_group_name
}

locals {
  location               = coalesce(var.location, data.azurerm_resource_group.this.location)
  storage_resource_group = coalesce(var.storage_resource_group_name, var.resource_group_name)
  github_issuer          = "https://token.actions.githubusercontent.com"
}

# Identities

resource "azurerm_user_assigned_identity" "writer" {
  name                = "${var.name_prefix}-ci-writer"
  resource_group_name = var.resource_group_name
  location            = local.location
}

resource "azurerm_user_assigned_identity" "reader" {
  name                = "${var.name_prefix}-ci-reader"
  resource_group_name = var.resource_group_name
  location            = local.location
}


resource "azurerm_federated_identity_credential" "writer" {
  for_each = var.writer_subjects

  name                = "github-${substr(sha256(each.value), 0, 13)}"
  resource_group_name = var.resource_group_name
  parent_id           = azurerm_user_assigned_identity.writer.id
  issuer              = local.github_issuer
  subject             = each.value
  audience            = ["api://AzureADTokenExchange"]
}

resource "azurerm_federated_identity_credential" "reader" {
  for_each = var.reader_subjects

  name                = "github-${substr(sha256(each.value), 0, 13)}"
  resource_group_name = var.resource_group_name
  parent_id           = azurerm_user_assigned_identity.reader.id
  issuer              = local.github_issuer
  subject             = each.value
  audience            = ["api://AzureADTokenExchange"]
}

# Storage

# Fails if the account already exists: this module owns it (import it to adopt one).
resource "azurerm_storage_account" "this" {
  name                     = var.storage_account_name
  resource_group_name      = local.storage_resource_group
  location                 = local.location
  account_kind             = "StorageV2"
  account_tier             = "Standard"
  account_replication_type = var.storage_sku
  access_tier              = "Hot"

  # The server only ever uses Entra identities: no account keys.
  shared_access_key_enabled       = false
  allow_nested_items_to_be_public = false
  min_tls_version                 = "TLS1_2"
  https_traffic_only_enabled      = true
  default_to_oauth_authentication = true
}

# Created through the management plane (storage_account_id), so it works without shared keys.
resource "azurerm_storage_container" "this" {
  name                  = var.container_name
  storage_account_id    = azurerm_storage_account.this.id
  container_access_type = "private"
}

locals {
  container_id = "${azurerm_storage_account.this.id}/blobServices/default/containers/${var.container_name}"

  # Keyed by static names: principal ids of the identities are unknown until apply.
  blob_access = merge(
    {
      "ci-writer" = { principal_id = azurerm_user_assigned_identity.writer.principal_id, role = "Storage Blob Data Contributor", type = "ServicePrincipal" }
      "ci-reader" = { principal_id = azurerm_user_assigned_identity.reader.principal_id, role = "Storage Blob Data Reader", type = "ServicePrincipal" }
    },
    { for id in var.reader_group_object_ids : "reader-group-${id}" => { principal_id = id, role = "Storage Blob Data Reader", type = "Group" } },
    { for id in var.writer_group_object_ids : "writer-group-${id}" => { principal_id = id, role = "Storage Blob Data Contributor", type = "Group" } },
  )
}

resource "azurerm_role_assignment" "blob_access" {
  for_each = local.blob_access

  scope                = local.container_id
  role_definition_name = each.value.role
  principal_id         = each.value.principal_id
  principal_type       = each.value.type

  depends_on = [azurerm_storage_container.this]
}

# An account has a single lifecycle policy document: writing it replaces any existing rules.
resource "azurerm_storage_management_policy" "this" {
  count = var.manage_lifecycle_policy ? 1 : 0

  storage_account_id = azurerm_storage_account.this.id

  rule {
    name    = "${var.name_prefix}-expire"
    enabled = true

    filters {
      blob_types   = ["blockBlob"]
      prefix_match = ["${var.container_name}/"]
    }

    actions {
      base_blob {
        delete_after_days_since_creation_greater_than = var.retention_days
      }
    }
  }
}
