//! Configuration: a TOML file plus `NX_CACHE__`-prefixed environment overrides
//! (`NX_CACHE__SERVER__LISTEN=0.0.0.0:8080`). Secrets are never part of the config itself;
//! config only names the environment variables that hold them.

use std::{net::SocketAddr, path::Path, sync::Arc};

use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::Deserialize;

use crate::{
    auth::{
        AuthChain, Authenticator, Scope,
        anonymous::Anonymous,
        entra::{EntraAuthenticator, EntraConfig},
        static_token::{StaticTokenEntry, StaticTokens},
    },
    store::{
        CacheStore,
        azure::{AzureBlobStore, AzureConfig},
        memory::MemoryStore,
    },
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    pub storage: StorageConfig,
    pub auth: AuthConfig,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    #[serde(default = "default_max_artifact_bytes")]
    pub max_artifact_bytes: u64,
    /// Emit JSON logs (for log aggregation) instead of human-readable ones.
    #[serde(default)]
    pub json_logs: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            max_artifact_bytes: default_max_artifact_bytes(),
            json_logs: false,
        }
    }
}

fn default_listen() -> SocketAddr {
    ([0, 0, 0, 0], 3000).into()
}

fn default_max_artifact_bytes() -> u64 {
    2 * 1024 * 1024 * 1024
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StorageConfig {
    Azure(Box<AzureConfig>),
    /// Not persistent; for local smoke tests only.
    Memory,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    pub providers: Vec<ProviderConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConfig {
    Entra(EntraConfig),
    Static {
        tokens: Vec<StaticTokenEntry>,
    },
    Anonymous {
        scope: Scope,
    },
    #[cfg(feature = "mint")]
    GithubMint(crate::auth::mint::MintConfig),
}

impl Config {
    /// Sources, later ones overriding earlier ones: the TOML file (optional when it is the
    /// default path), inline TOML in `NX_CACHE_CONFIG_TOML` (convenient for containers, where
    /// mounting a file is awkward), then `NX_CACHE__*` overrides.
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut figment = Figment::new();
        if path.exists() {
            figment = figment.merge(Toml::file(path));
        } else if path != Path::new(DEFAULT_CONFIG_PATH) {
            return Err(format!("config file {} does not exist", path.display()));
        }
        if let Ok(inline) = std::env::var(INLINE_CONFIG_ENV) {
            figment = figment.merge(Toml::string(&inline));
        }
        figment
            .merge(Env::prefixed("NX_CACHE__").split("__"))
            .extract()
            .map_err(|e| format!("invalid configuration: {e}"))
    }

    pub fn build_store(&self) -> Result<Arc<dyn CacheStore>, String> {
        Ok(match &self.storage {
            StorageConfig::Azure(azure) => {
                let store = AzureBlobStore::new(azure)?;
                tracing::info!(storage = store.describe(), "using Azure Blob storage");
                Arc::new(store)
            }
            StorageConfig::Memory => {
                tracing::warn!("using in-memory storage: nothing is persisted");
                Arc::new(MemoryStore::new())
            }
        })
    }

    pub async fn build_auth(&self) -> Result<Built, String> {
        if self.auth.providers.is_empty() {
            return Err("auth.providers is empty: every request would be rejected".into());
        }
        let mut providers: Vec<Arc<dyn Authenticator>> = Vec::new();
        #[cfg(feature = "mint")]
        let mut minter = None;
        for provider in &self.auth.providers {
            match provider {
                ProviderConfig::Entra(config) => {
                    tracing::info!(issuer = config.issuer(), "auth provider: entra");
                    providers.push(Arc::new(
                        EntraAuthenticator::discover(config.clone()).await?,
                    ));
                }
                ProviderConfig::Static { tokens } => {
                    tracing::info!(count = tokens.len(), "auth provider: static tokens");
                    providers.push(Arc::new(StaticTokens::new(tokens.clone())?));
                }
                ProviderConfig::Anonymous { scope } => {
                    if *scope != Scope::Read {
                        return Err("anonymous access can only be granted `read` scope".into());
                    }
                    tracing::info!("auth provider: anonymous read");
                    providers.push(Arc::new(Anonymous { scope: *scope }));
                }
                #[cfg(feature = "mint")]
                ProviderConfig::GithubMint(config) => {
                    if minter.is_some() {
                        return Err("only one github_mint provider may be configured".into());
                    }
                    let built =
                        Arc::new(crate::auth::mint::Minter::discover(config.clone()).await?);
                    tracing::info!("auth provider: github_mint");
                    providers.push(built.clone());
                    minter = Some(built);
                }
            }
        }
        Ok(Built {
            chain: AuthChain::new(providers),
            #[cfg(feature = "mint")]
            minter,
        })
    }
}

pub struct Built {
    pub chain: AuthChain,
    #[cfg(feature = "mint")]
    pub minter: Option<Arc<crate::auth::mint::Minter>>,
}

pub const DEFAULT_CONFIG_PATH: &str = "config.toml";
pub const INLINE_CONFIG_ENV: &str = "NX_CACHE_CONFIG_TOML";

#[cfg(test)]
mod tests {
    use super::*;
    use figment::providers::Format;

    fn parse(toml: &str) -> Result<Config, Box<figment::Error>> {
        Figment::from(Toml::string(toml))
            .extract()
            .map_err(Box::new)
    }

    #[test]
    fn parses_full_example() {
        let config = parse(
            r#"
            [server]
            listen = "127.0.0.1:8080"

            [storage]
            type = "azure"
            account = "daffodilnx"
            container = "nx-cache-v1"
            auth = { mode = "managed_identity" }

            [[auth.providers]]
            type = "entra"
            tenant_id = "5e3d1041-a3d4-4599-a4e0-6b35da52825a"
            audiences = ["00000000-0000-0000-0000-000000000001"]

            [[auth.providers]]
            type = "static"
            tokens = [{ name = "dev", sha256 = "00", scope = "read" }]

            [[auth.providers]]
            type = "anonymous"
            scope = "read"
            "#,
        )
        .unwrap();
        assert_eq!(config.server.listen.port(), 8080);
        let StorageConfig::Azure(azure) = &config.storage else {
            panic!()
        };
        assert_eq!(azure.prefix, "v1");
        assert_eq!(config.auth.providers.len(), 3);
    }

    #[test]
    fn parses_bicep_generated_config() {
        // Mirrors `configLines` in deploy/main.bicep, with every optional provider enabled.
        let config = parse(
            r#"[server]
listen = "0.0.0.0:3000"
json_logs = true
[storage]
type = "azure"
account = "daffodilnx"
container = "nx-cache-v1"
auth = { mode = "managed_identity", client_id = "00000000-0000-0000-0000-000000000002" }
[[auth.providers]]
type = "entra"
tenant_id = "00000000-0000-0000-0000-000000000003"
audiences = ["00000000-0000-0000-0000-000000000004"]
[[auth.providers]]
type = "static"
tokens = [{ name = "dev-readonly", sha256 = "00", scope = "read" }]
[[auth.providers]]
type = "anonymous"
scope = "read"
"#,
        )
        .unwrap();
        assert!(config.server.json_logs);
        assert_eq!(config.auth.providers.len(), 3);
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = parse(
            r#"
            [storage]
            type = "memory"
            [auth]
            providers = []
            typo = true
            "#,
        );
        assert!(err.is_err());
    }

    #[test]
    fn rejects_unknown_storage_auth_mode() {
        let err = parse(
            r#"
            [storage]
            type = "azure"
            account = "x"
            container = "c"
            auth = { mode = "connection_string" }
            [auth]
            providers = []
            "#,
        );
        assert!(err.is_err());
    }
}
