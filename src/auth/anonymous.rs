//! Requests without a bearer token. Off unless configured; meant for read-only access to a
//! public project's cache (e.g. fork PRs, which get neither secrets nor an OIDC token).

use async_trait::async_trait;

use super::{AuthError, Authenticator, Presented, Principal, Scope};

pub struct Anonymous {
    pub scope: Scope,
}

#[async_trait]
impl Authenticator for Anonymous {
    async fn authenticate(
        &self,
        presented: &Presented<'_>,
    ) -> Result<Option<Principal>, AuthError> {
        Ok(matches!(presented, Presented::None).then(|| Principal {
            provider: "anonymous",
            subject: "anonymous".into(),
            scope: self.scope,
            meta: Default::default(),
        }))
    }
}
