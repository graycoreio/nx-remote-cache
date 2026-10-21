//! Client → server authentication.
//!
//! Every provider implements [`Authenticator`]. [`AuthChain`] classifies the presented bearer
//! once (none / opaque / JWT with an *unverified* issuer) and offers it to each configured
//! provider in order. The unverified issuer is only used to let providers skip tokens that are
//! obviously not theirs; the provider that claims a token always verifies it fully.

pub mod anonymous;
pub mod entra;
#[cfg(feature = "mint")]
pub mod mint;
pub mod static_token;

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Read,
    ReadWrite,
}

impl Scope {
    pub fn allows(self, required: Scope) -> bool {
        self >= required
    }
}

#[derive(Clone, Debug)]
pub struct Principal {
    pub provider: &'static str,
    pub subject: String,
    pub scope: Scope,
    /// Recorded as provenance on anything this principal writes.
    pub meta: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid token: {0}")]
    Invalid(String),
    #[error("token is valid but grants no cache access")]
    NoAccess,
}

/// What the client presented, classified once by the chain.
pub enum Presented<'a> {
    None,
    Opaque(&'a str),
    /// A JWT-shaped bearer. `issuer` is read from the payload WITHOUT verification.
    Jwt {
        token: &'a str,
        issuer: String,
    },
}

impl<'a> Presented<'a> {
    pub fn from_bearer(bearer: Option<&'a str>) -> Self {
        match bearer {
            None => Self::None,
            Some(token) => match unverified_issuer(token) {
                Some(issuer) => Self::Jwt { token, issuer },
                None => Self::Opaque(token),
            },
        }
    }
}

fn unverified_issuer(token: &str) -> Option<String> {
    let mut parts = token.split('.');
    let (_header, payload, _sig) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    #[derive(Deserialize)]
    struct Iss {
        iss: String,
    }
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice::<Iss>(&bytes).ok().map(|c| c.iss)
}

/// Build a `jwt-authorizer` from OpenID discovery, retrying with backoff so a transient network
/// blip at startup doesn't crash the server (discovery only happens once, at startup).
pub(crate) async fn discover<C>(
    issuer: &str,
    validation: impl Fn() -> jwt_authorizer::Validation,
) -> Result<jwt_authorizer::Authorizer<C>, String>
where
    C: serde::de::DeserializeOwned + Clone + Send + Sync + 'static,
{
    const ATTEMPTS: u32 = 5;
    let mut delay = std::time::Duration::from_secs(1);
    for attempt in 1..=ATTEMPTS {
        match jwt_authorizer::JwtAuthorizer::<C>::from_oidc(issuer)
            .validation(validation())
            .build()
            .await
        {
            Ok(authorizer) => return Ok(authorizer),
            Err(e) if attempt < ATTEMPTS => {
                tracing::warn!(issuer, attempt, error = %e, "OpenID discovery failed; retrying");
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(e) => return Err(format!("OpenID discovery for {issuer} failed: {e}")),
        }
    }
    unreachable!()
}

#[async_trait]
pub trait Authenticator: Send + Sync {
    /// `Ok(None)`: not this provider's kind of token, try the next one.
    /// `Err`: this provider's token, but it is invalid; stop and reject.
    async fn authenticate(&self, presented: &Presented<'_>)
    -> Result<Option<Principal>, AuthError>;
}

#[derive(Clone, Default)]
pub struct AuthChain {
    providers: Vec<Arc<dyn Authenticator>>,
}

impl AuthChain {
    pub fn new(providers: Vec<Arc<dyn Authenticator>>) -> Self {
        Self { providers }
    }

    pub async fn authenticate(&self, bearer: Option<&str>) -> Result<Principal, AuthError> {
        let presented = Presented::from_bearer(bearer);
        for provider in &self.providers {
            if let Some(principal) = provider.authenticate(&presented).await? {
                return Ok(principal);
            }
        }
        Err(AuthError::Invalid(match presented {
            Presented::None => "missing bearer token".into(),
            Presented::Opaque(_) => "unknown token".into(),
            Presented::Jwt { issuer, .. } => {
                format!("no provider accepts tokens from issuer {issuer}")
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_bearers() {
        assert!(matches!(Presented::from_bearer(None), Presented::None));
        assert!(matches!(
            Presented::from_bearer(Some("abc123")),
            Presented::Opaque(_)
        ));
        let payload = URL_SAFE_NO_PAD.encode(br#"{"iss":"https://issuer.example"}"#);
        let jwt = format!("e30.{payload}.sig");
        match Presented::from_bearer(Some(&jwt)) {
            Presented::Jwt { issuer, .. } => assert_eq!(issuer, "https://issuer.example"),
            _ => panic!("expected a JWT"),
        }
        assert!(matches!(
            Presented::from_bearer(Some("a.b.c")),
            Presented::Opaque(_)
        ));
    }

    #[test]
    fn scope_ordering() {
        assert!(Scope::ReadWrite.allows(Scope::Read));
        assert!(Scope::Read.allows(Scope::Read));
        assert!(!Scope::Read.allows(Scope::ReadWrite));
    }
}
