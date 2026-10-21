//! GitHub Actions OIDC claims and the rules that map them to a [`Scope`].

use serde::Deserialize;

use crate::auth::Scope;

pub const GITHUB_ISSUER: &str = "https://token.actions.githubusercontent.com";

#[derive(Clone, Debug, Deserialize)]
pub struct GithubClaims {
    pub sub: String,
    /// Numeric, but GitHub sends it as a string.
    pub repository_id: String,
    pub repository: String,
    pub event_name: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub run_id: Option<String>,
    pub sha: Option<String>,
    pub workflow_ref: Option<String>,
}

/// First matching rule wins; no match means no access. Rules pin `repository_id`, not the
/// repository name, so a renamed or re-created repository cannot inherit access.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub repository_id: u64,
    /// If set, the token's `event_name` must be one of these (e.g. `["push"]`).
    pub event_name: Option<Vec<String>>,
    /// If set, the token's `ref` must be one of these (e.g. `["refs/heads/develop"]`).
    #[serde(rename = "ref")]
    pub git_ref: Option<Vec<String>>,
    pub scope: Scope,
}

impl Rule {
    fn matches(&self, claims: &GithubClaims) -> bool {
        claims.repository_id == self.repository_id.to_string()
            && self
                .event_name
                .as_ref()
                .is_none_or(|e| e.contains(&claims.event_name))
            && self
                .git_ref
                .as_ref()
                .is_none_or(|r| r.contains(&claims.git_ref))
    }
}

pub fn scope_for(rules: &[Rule], claims: &GithubClaims) -> Option<Scope> {
    rules.iter().find(|r| r.matches(claims)).map(|r| r.scope)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(event: &str, git_ref: &str, repo_id: &str) -> GithubClaims {
        GithubClaims {
            sub: "repo:graycoreio/daffodil:ref:refs/heads/develop".into(),
            repository_id: repo_id.into(),
            repository: "graycoreio/daffodil".into(),
            event_name: event.into(),
            git_ref: git_ref.into(),
            run_id: None,
            sha: None,
            workflow_ref: None,
        }
    }

    fn rules() -> Vec<Rule> {
        vec![
            Rule {
                repository_id: 42,
                event_name: Some(vec!["push".into()]),
                git_ref: Some(vec!["refs/heads/develop".into()]),
                scope: Scope::ReadWrite,
            },
            Rule {
                repository_id: 42,
                event_name: None,
                git_ref: None,
                scope: Scope::Read,
            },
        ]
    }

    #[test]
    fn push_to_develop_writes_everything_else_reads() {
        let r = rules();
        assert_eq!(
            scope_for(&r, &claims("push", "refs/heads/develop", "42")),
            Some(Scope::ReadWrite)
        );
        assert_eq!(
            scope_for(&r, &claims("pull_request", "refs/pull/1/merge", "42")),
            Some(Scope::Read)
        );
        assert_eq!(
            scope_for(&r, &claims("push", "refs/heads/feature", "42")),
            Some(Scope::Read)
        );
        assert_eq!(
            scope_for(&r, &claims("workflow_dispatch", "refs/heads/develop", "42")),
            Some(Scope::Read)
        );
    }

    #[test]
    fn other_repositories_get_nothing() {
        assert_eq!(
            scope_for(&rules(), &claims("push", "refs/heads/develop", "43")),
            None
        );
        assert_eq!(
            scope_for(&rules(), &claims("push", "refs/heads/develop", "420")),
            None
        );
    }
}
