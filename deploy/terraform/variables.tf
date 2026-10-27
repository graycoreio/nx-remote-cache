variable "resource_group_name" {
  description = "Resource group for the CI identities (and the storage, unless storage_resource_group_name is set)."
  type        = string
}

variable "name_prefix" {
  description = "Prefix for the identity names."
  type        = string
  default     = "nx-remote-cache"
}

variable "location" {
  description = "Defaults to the resource group's location."
  type        = string
  default     = null
}

variable "storage_account_name" {
  description = "Storage account that holds the cache, created by this module (locked down: no shared keys, no public access). Must not already exist."
  type        = string

  validation {
    condition     = can(regex("^[a-z0-9]{3,24}$", var.storage_account_name))
    error_message = "3-24 lowercase letters and digits."
  }
}

variable "storage_resource_group_name" {
  description = "Resource group of the storage account. Defaults to resource_group_name."
  type        = string
  default     = null
}

variable "storage_sku" {
  description = "Replication for the account. Cache entries are reproducible, so LRS is enough."
  type        = string
  default     = "LRS"
}

variable "container_name" {
  description = "Blob container for cache entries."
  type        = string
  default     = "nx-cache-v1"
}

variable "retention_days" {
  description = "Cache entries are deleted this many days after creation."
  type        = number
  default     = 30
}

variable "manage_lifecycle_policy" {
  description = <<-EOT
    Delete cache entries retention_days after creation, through the account's lifecycle management
    policy. An account has a single policy document, so this replaces any rules added outside the
    module.
  EOT
  type        = bool
  default     = true
}

variable "writer_subjects" {
  description = "GitHub OIDC subjects allowed to WRITE (exact match, no wildcards), e.g. repo:graycoreio/daffodil:ref:refs/heads/develop"
  type        = set(string)
}

variable "reader_subjects" {
  description = "GitHub OIDC subjects allowed to READ, e.g. repo:graycoreio/daffodil:pull_request"
  type        = set(string)
}

variable "reader_group_object_ids" {
  description = "Entra group object ids whose members may READ the cache from their own machines (Storage Blob Data Reader on the container)."
  type        = set(string)
  default     = []
}

variable "writer_group_object_ids" {
  description = "Entra group object ids whose members may also WRITE from their own machines (Storage Blob Data Contributor). Entries they write are served to CI; keep this group small."
  type        = set(string)
  default     = []
}
