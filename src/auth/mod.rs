//! Client → server authentication.
//!
//! Every provider implements [`Authenticator`]; [`AuthChain`] offers the presented bearer to
//! each configured provider in order. Static tokens are the only provider: the server runs next
//! to whoever uses it (a CI job, a developer's machine), so callers get a token from whoever
//! started it, and storage access is the caller's own identity.

pub mod static_token;

use std::sync::Arc;

use async_trait::async_trait;
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
    /// The token's configured name; recorded on everything it writes.
    pub name: String,
    pub scope: Scope,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid token: {0}")]
    Invalid(String),
}

/// What the client presented.
pub enum Presented<'a> {
    None,
    Bearer(&'a str),
}

impl<'a> Presented<'a> {
    pub fn from_bearer(bearer: Option<&'a str>) -> Self {
        bearer.map_or(Self::None, Self::Bearer)
    }
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
            Presented::Bearer(_) => "unknown token".into(),
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
            Presented::Bearer("abc123")
        ));
    }

    #[test]
    fn scope_ordering() {
        assert!(Scope::ReadWrite.allows(Scope::Read));
        assert!(Scope::Read.allows(Scope::Read));
        assert!(!Scope::Read.allows(Scope::ReadWrite));
    }
}
