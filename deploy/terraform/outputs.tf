# Inputs for graycoreio/github-actions/nx-remote-cache.

output "tenant_id" {
  value = data.azurerm_client_config.current.tenant_id
}

output "storage_account" {
  value = var.storage_account_name
}

output "storage_container" {
  value = var.container_name
}

output "writer_client_id" {
  value = azurerm_user_assigned_identity.writer.client_id
}

output "reader_client_id" {
  value = azurerm_user_assigned_identity.reader.client_id
}
