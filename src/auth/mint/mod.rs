//! Server-minted tokens (feature `mint`).
//!
//! `POST /v1/token/github` exchanges a short-lived GitHub Actions OIDC token for a longer-lived
//! token signed by this server, so an Nx run can outlast GitHub's ~5 minute token. Unlike the
//! Entra provider, this means the server owns signing keys and the rules deciding who may
//! write, which is why it is not compiled in by default.

pub mod github;

use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use jwt_authorizer::Authorizer;
use serde::{Deserialize, Serialize};

use self::github::{GITHUB_ISSUER, GithubClaims, Rule, scope_for};
use super::{AuthError, Authenticator, Presented, Principal, Scope};

/// Below this, an HMAC key is guessable offline from any token it signed.
const MIN_SECRET_BYTES: usize = 32;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MintConfig {
    /// The `aud` CI must request for its GitHub OIDC token (e.g. the server's URL).
    pub github_audience: String,
    /// `iss`/`aud` of the tokens this server mints. Must differ from every other provider's issuer.
    #[serde(default = "default_issuer")]
    pub issuer: String,
    /// The first key signs; all keys verify. Rotate by prepending a new key, then removing the
    /// old one after `ttl` has passed.
    pub signing_keys: Vec<SigningKeyConfig>,
    #[serde(default = "default_ttl", with = "humantime_serde")]
    pub ttl: Duration,
    pub rules: Vec<Rule>,
    /// Override for tests only.
    #[serde(default = "default_github_issuer")]
    pub github_issuer: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningKeyConfig {
    pub kid: String,
    /// Environment variable holding the secret (at least 32 bytes).
    pub secret_env: String,
}

fn default_issuer() -> String {
    "nx-remote-cache".into()
}

fn default_ttl() -> Duration {
    Duration::from_secs(2 * 60 * 60)
}

fn default_github_issuer() -> String {
    GITHUB_ISSUER.into()
}

#[derive(Debug, Serialize, Deserialize)]
struct MintedClaims {
    iss: String,
    aud: String,
    sub: String,
    iat: u64,
    nbf: u64,
    exp: u64,
    scope: Scope,
    #[serde(default)]
    meta: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct MintedToken {
    pub token: String,
    pub scope: Scope,
    pub expires_in: u64,
}

pub struct Minter {
    config: MintConfig,
    github: Authorizer<GithubClaims>,
    keys: Vec<(String, EncodingKey, DecodingKey)>,
}

impl Minter {
    pub async fn discover(config: MintConfig) -> Result<Self, String> {
        let keys = load_keys(&config)?;
        let github = super::discover(&config.github_issuer, || github_validation(&config))
            .await
            .map_err(|e| format!("github_mint provider: {e}"))?;
        Self::new(config, github, keys)
    }

    /// For tests: verify GitHub tokens against a local JWKS and use the given secrets.
    pub async fn from_jwks_text(
        config: MintConfig,
        jwks: &str,
        secrets: &[(&str, &[u8])],
    ) -> Result<Self, String> {
        let github = jwt_authorizer::JwtAuthorizer::from_jwks_text(jwks)
            .validation(github_validation(&config))
            .build()
            .await
            .map_err(|e| e.to_string())?;
        let keys = secrets
            .iter()
            .map(|(kid, secret)| key_pair(kid, secret))
            .collect::<Result<_, _>>()?;
        Self::new(config, github, keys)
    }

    fn new(
        config: MintConfig,
        github: Authorizer<GithubClaims>,
        keys: Vec<(String, EncodingKey, DecodingKey)>,
    ) -> Result<Self, String> {
        if keys.is_empty() {
            return Err("github_mint provider: at least one signing key is required".into());
        }
        if config.rules.is_empty() {
            return Err("github_mint provider: no rules, so no token would ever be minted".into());
        }
        if config.issuer == config.github_issuer {
            return Err("github_mint provider: `issuer` must differ from the GitHub issuer".into());
        }
        Ok(Self {
            config,
            github,
            keys,
        })
    }

    /// Verify a GitHub OIDC token, apply the rules and mint a token for the resulting scope.
    pub async fn exchange(&self, github_token: &str) -> Result<MintedToken, AuthError> {
        let claims = self
            .github
            .check_auth(github_token)
            .await
            .map_err(|e| AuthError::Invalid(format!("GitHub token rejected: {e}")))?
            .claims;
        let scope = scope_for(&self.config.rules, &claims).ok_or(AuthError::NoAccess)?;

        let now = unix_now();
        let ttl = self.config.ttl.as_secs();
        let mut meta = BTreeMap::from([
            ("provider".to_owned(), "github_mint".to_owned()),
            ("subject".to_owned(), claims.sub.clone()),
            ("repository".to_owned(), claims.repository.clone()),
            ("event".to_owned(), claims.event_name.clone()),
            ("ref".to_owned(), claims.git_ref.clone()),
        ]);
        for (k, v) in [
            ("run_id", &claims.run_id),
            ("sha", &claims.sha),
            ("workflow_ref", &claims.workflow_ref),
        ] {
            if let Some(v) = v {
                meta.insert(k.to_owned(), v.clone());
            }
        }
        let minted = MintedClaims {
            iss: self.config.issuer.clone(),
            aud: self.config.issuer.clone(),
            sub: claims.sub,
            iat: now,
            nbf: now,
            exp: now + ttl,
            scope,
            meta,
        };
        let (kid, encoding, _) = &self.keys[0];
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(kid.clone());
        let token = jsonwebtoken::encode(&header, &minted, encoding)
            .map_err(|e| AuthError::Invalid(format!("signing failed: {e}")))?;
        Ok(MintedToken {
            token,
            scope,
            expires_in: ttl,
        })
    }
}

#[async_trait]
impl Authenticator for Minter {
    async fn authenticate(
        &self,
        presented: &Presented<'_>,
    ) -> Result<Option<Principal>, AuthError> {
        let Presented::Jwt { token, issuer } = presented else {
            return Ok(None);
        };
        if *issuer != self.config.issuer {
            return Ok(None);
        }
        let header =
            jsonwebtoken::decode_header(token).map_err(|e| AuthError::Invalid(e.to_string()))?;
        let kid = header
            .kid
            .ok_or_else(|| AuthError::Invalid("minted token has no kid".into()))?;
        let (_, _, decoding) = self
            .keys
            .iter()
            .find(|(k, _, _)| *k == kid)
            .ok_or_else(|| AuthError::Invalid(format!("unknown signing key {kid}")))?;

        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[&self.config.issuer]);
        validation.set_audience(&[&self.config.issuer]);
        validation.validate_nbf = true;
        let claims = jsonwebtoken::decode::<MintedClaims>(token, decoding, &validation)
            .map_err(|e| AuthError::Invalid(e.to_string()))?
            .claims;
        Ok(Some(Principal {
            provider: "github_mint",
            subject: claims.sub,
            scope: claims.scope,
            meta: claims.meta,
        }))
    }
}

fn github_validation(config: &MintConfig) -> jwt_authorizer::Validation {
    jwt_authorizer::Validation::new()
        .iss(&[&config.github_issuer])
        .aud(&[&config.github_audience])
        .nbf(true)
}

fn load_keys(config: &MintConfig) -> Result<Vec<(String, EncodingKey, DecodingKey)>, String> {
    config
        .signing_keys
        .iter()
        .map(|k| {
            let secret = std::env::var(&k.secret_env).map_err(|_| {
                format!(
                    "github_mint provider: signing key {:?} needs environment variable {}",
                    k.kid, k.secret_env
                )
            })?;
            key_pair(&k.kid, secret.as_bytes())
        })
        .collect()
}

fn key_pair(kid: &str, secret: &[u8]) -> Result<(String, EncodingKey, DecodingKey), String> {
    if secret.len() < MIN_SECRET_BYTES {
        return Err(format!(
            "github_mint provider: signing key {kid:?} must be at least {MIN_SECRET_BYTES} bytes"
        ));
    }
    Ok((
        kid.to_owned(),
        EncodingKey::from_secret(secret),
        DecodingKey::from_secret(secret),
    ))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
