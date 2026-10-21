//! Microsoft Entra ID access tokens.
//!
//! Entra issues and signs the tokens; who may obtain a write token is decided by exact-match
//! federated identity credentials in Azure, not by code here. This provider only verifies the
//! token (via `jwt-authorizer`: discovery, JWKS cache with refresh on unknown `kid`, signature,
//! `iss`, `aud`, `exp`, `nbf`) and maps the `roles` claim to a [`Scope`].
//!
//! The API's app registration must issue v2 tokens (`requestedAccessTokenVersion: 2`); v2 tokens
//! carry the API's client id (a GUID) in `aud`.

use async_trait::async_trait;
use jwt_authorizer::{Authorizer, JwtAuthorizer, Validation};
use serde::Deserialize;

use super::{AuthError, Authenticator, Presented, Principal, Scope};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntraConfig {
    pub tenant_id: String,
    /// Accepted `aud` values: the API app registration's client id (v2 tokens).
    pub audiences: Vec<String>,
    #[serde(default = "default_read_role")]
    pub read_role: String,
    #[serde(default = "default_write_role")]
    pub write_role: String,
    #[serde(default = "default_authority")]
    pub authority: String,
}

fn default_read_role() -> String {
    "Cache.Read".into()
}

fn default_write_role() -> String {
    "Cache.Write".into()
}

fn default_authority() -> String {
    "https://login.microsoftonline.com".into()
}

impl EntraConfig {
    pub fn issuer(&self) -> String {
        format!(
            "{}/{}/v2.0",
            self.authority.trim_end_matches('/'),
            self.tenant_id
        )
    }

    pub fn validation(&self) -> Validation {
        // jwt-authorizer checks `exp` by default but not `nbf`.
        Validation::new()
            .iss(&[self.issuer()])
            .aud(&self.audiences)
            .nbf(true)
    }
}

#[derive(Clone, Debug, Deserialize)]
struct EntraClaims {
    tid: String,
    sub: String,
    oid: Option<String>,
    azp: Option<String>,
    #[serde(default)]
    roles: Vec<String>,
}

pub struct EntraAuthenticator {
    config: EntraConfig,
    issuer: String,
    authorizer: Authorizer<EntraClaims>,
}

impl EntraAuthenticator {
    /// Fetches the OpenID configuration for the tenant (network call at startup).
    pub async fn discover(config: EntraConfig) -> Result<Self, String> {
        if config.audiences.is_empty() {
            return Err("entra provider: `audiences` must not be empty".into());
        }
        let authorizer = super::discover(&config.issuer(), || config.validation())
            .await
            .map_err(|e| format!("entra provider: {e}"))?;
        Ok(Self::with_authorizer(config, authorizer))
    }

    /// For tests: supply an authorizer built from a local JWKS.
    pub async fn from_jwks_text(config: EntraConfig, jwks: &str) -> Result<Self, String> {
        let authorizer = JwtAuthorizer::from_jwks_text(jwks)
            .validation(config.validation())
            .build()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Self::with_authorizer(config, authorizer))
    }

    fn with_authorizer(config: EntraConfig, authorizer: Authorizer<EntraClaims>) -> Self {
        Self {
            issuer: config.issuer(),
            config,
            authorizer,
        }
    }
}

#[async_trait]
impl Authenticator for EntraAuthenticator {
    async fn authenticate(
        &self,
        presented: &Presented<'_>,
    ) -> Result<Option<Principal>, AuthError> {
        let Presented::Jwt { token, issuer } = presented else {
            return Ok(None);
        };
        if *issuer != self.issuer {
            return Ok(None);
        }
        let claims = self
            .authorizer
            .check_auth(token)
            .await
            .map_err(|e| AuthError::Invalid(e.to_string()))?
            .claims;
        // `iss` already pins the tenant; checking `tid` too costs nothing.
        if claims.tid != self.config.tenant_id {
            return Err(AuthError::Invalid("token is from another tenant".into()));
        }
        let scope = if claims.roles.contains(&self.config.write_role) {
            Scope::ReadWrite
        } else if claims.roles.contains(&self.config.read_role) {
            Scope::Read
        } else {
            return Err(AuthError::NoAccess);
        };
        let subject = claims.oid.clone().unwrap_or_else(|| claims.sub.clone());
        let mut meta = std::collections::BTreeMap::from([
            ("provider".to_owned(), "entra".to_owned()),
            ("subject".to_owned(), subject.clone()),
        ]);
        if let Some(azp) = claims.azp {
            meta.insert("client_id".into(), azp);
        }
        Ok(Some(Principal {
            provider: "entra",
            subject,
            scope,
            meta,
        }))
    }
}
