//! Server → Azure Blob auth. Exactly one explicitly configured mode, never a guessing chain:
//! when something is wrong the error names the mode that was tried.

use std::sync::Arc;

use azure_core::credentials::{Secret, TokenCredential};
use azure_identity::{
    ClientSecretCredential, DeveloperToolsCredential, ManagedIdentityCredential,
    ManagedIdentityCredentialOptions, UserAssignedId, WorkloadIdentityCredential,
    WorkloadIdentityCredentialOptions,
};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum StorageAuth {
    /// Azure Container Apps / App Service / VMs. Omit `client_id` for a system-assigned identity.
    ManagedIdentity { client_id: Option<String> },
    /// AKS-style federation; reads `AZURE_FEDERATED_TOKEN_FILE` etc. unless overridden.
    WorkloadIdentity {
        tenant_id: Option<String>,
        client_id: Option<String>,
    },
    /// A service principal. The secret is read from the named environment variable.
    ClientSecret {
        tenant_id: String,
        client_id: String,
        #[serde(default = "default_client_secret_env")]
        client_secret_env: String,
    },
    /// The developer's own `az login` / `azd auth login`. For `cargo run` on a dev machine.
    DeveloperTools,
    /// A SAS token (ideally a user-delegation SAS) read from the named environment variable.
    /// The only mode that works over plain http, i.e. against Azurite.
    Sas {
        #[serde(default = "default_sas_env")]
        sas_token_env: String,
    },
}

fn default_client_secret_env() -> String {
    "AZURE_CLIENT_SECRET".into()
}

fn default_sas_env() -> String {
    "AZURE_STORAGE_SAS_TOKEN".into()
}

pub enum ResolvedAuth {
    Credential(Arc<dyn TokenCredential>),
    Sas(String),
}

impl StorageAuth {
    pub fn describe(&self) -> &'static str {
        match self {
            Self::ManagedIdentity { .. } => "managed_identity",
            Self::WorkloadIdentity { .. } => "workload_identity",
            Self::ClientSecret { .. } => "client_secret",
            Self::DeveloperTools => "developer_tools",
            Self::Sas { .. } => "sas",
        }
    }

    pub fn resolve(&self) -> Result<ResolvedAuth, String> {
        let mode = self.describe();
        let wrap = |e: azure_core::Error| format!("storage auth mode `{mode}`: {e}");
        Ok(match self {
            Self::ManagedIdentity { client_id } => {
                let options = ManagedIdentityCredentialOptions {
                    user_assigned_id: client_id.clone().map(UserAssignedId::ClientId),
                    ..Default::default()
                };
                ResolvedAuth::Credential(
                    ManagedIdentityCredential::new(Some(options)).map_err(wrap)?,
                )
            }
            Self::WorkloadIdentity {
                tenant_id,
                client_id,
            } => {
                let options = WorkloadIdentityCredentialOptions {
                    tenant_id: tenant_id.clone(),
                    client_id: client_id.clone(),
                    ..Default::default()
                };
                ResolvedAuth::Credential(
                    WorkloadIdentityCredential::new(Some(options)).map_err(wrap)?,
                )
            }
            Self::ClientSecret {
                tenant_id,
                client_id,
                client_secret_env,
            } => {
                let secret = read_env(client_secret_env, mode)?;
                ResolvedAuth::Credential(
                    ClientSecretCredential::new(
                        tenant_id,
                        client_id.clone(),
                        Secret::new(secret),
                        None,
                    )
                    .map_err(wrap)?,
                )
            }
            Self::DeveloperTools => {
                ResolvedAuth::Credential(DeveloperToolsCredential::new(None).map_err(wrap)?)
            }
            Self::Sas { sas_token_env } => {
                let token = read_env(sas_token_env, mode)?;
                ResolvedAuth::Sas(token.trim_start_matches('?').to_owned())
            }
        })
    }
}

fn read_env(name: &str, mode: &str) -> Result<String, String> {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            format!("storage auth mode `{mode}` requires the environment variable {name}")
        })
}
