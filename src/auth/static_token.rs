//! Long-lived tokens configured by their SHA-256 hash (the plaintext never touches config).
//! Intended for a shared developer read-only token and an optional break-glass writer.

use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::{AuthError, Authenticator, Presented, Principal, Scope};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaticTokenEntry {
    /// Shown in logs and recorded as the writer; never the token itself.
    pub name: String,
    /// Lowercase hex SHA-256 of the token. Generate with `nx-remote-cache generate-token`.
    pub sha256: String,
    pub scope: Scope,
}

pub struct StaticTokens {
    entries: Vec<([u8; 32], StaticTokenEntry)>,
}

impl StaticTokens {
    pub fn new(entries: Vec<StaticTokenEntry>) -> Result<Self, String> {
        let entries = entries
            .into_iter()
            .map(|entry| {
                let digest: [u8; 32] = hex::decode(entry.sha256.trim())
                    .ok()
                    .and_then(|d| d.try_into().ok())
                    .ok_or_else(|| {
                        format!(
                            "static token {:?}: sha256 must be 64 hex characters",
                            entry.name
                        )
                    })?;
                Ok((digest, entry))
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { entries })
    }
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[async_trait]
impl Authenticator for StaticTokens {
    async fn authenticate(
        &self,
        presented: &Presented<'_>,
    ) -> Result<Option<Principal>, AuthError> {
        let Presented::Opaque(token) = presented else {
            return Ok(None);
        };
        let digest = Sha256::digest(token.as_bytes());
        // Compare against every entry so timing doesn't reveal which (if any) matched.
        let mut matched = None;
        for (expected, entry) in &self.entries {
            if bool::from(expected.ct_eq(digest.as_slice())) {
                matched = Some(entry);
            }
        }
        Ok(matched.map(|entry| Principal {
            provider: "static",
            subject: entry.name.clone(),
            scope: entry.scope,
            meta: [("token".to_owned(), entry.name.clone())].into(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn matches_by_hash() {
        let tokens = StaticTokens::new(vec![StaticTokenEntry {
            name: "dev".into(),
            sha256: hash_token("s3cret"),
            scope: Scope::Read,
        }])
        .unwrap();
        let p = tokens
            .authenticate(&Presented::Opaque("s3cret"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!((p.subject.as_str(), p.scope), ("dev", Scope::Read));
        assert!(
            tokens
                .authenticate(&Presented::Opaque("nope"))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            tokens
                .authenticate(&Presented::None)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rejects_malformed_hash() {
        let err = StaticTokens::new(vec![StaticTokenEntry {
            name: "x".into(),
            sha256: "abc".into(),
            scope: Scope::Read,
        }]);
        assert!(err.is_err());
    }
}
