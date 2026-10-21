//! The `mint` provider: GitHub OIDC → server-minted token exchange. `cargo test --features mint`.
#![cfg(feature = "mint")]

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use nx_remote_cache::{
    auth::{
        AuthChain, Authenticator,
        mint::{MintConfig, Minter},
    },
    http::{App, AppState, app},
    store::{CacheKey, memory::MemoryStore},
};
use serde_json::{Value, json};
use tower::ServiceExt;

const REPO_ID: u64 = 123456;
const AUDIENCE: &str = "https://nx-cache.example";
const OLD_SECRET: &[u8] = b"old-secret-old-secret-old-secret-0";
const NEW_SECRET: &[u8] = b"new-secret-new-secret-new-secret-1";
const JWKS: &str = include_str!("fixtures/test_jwks.json");

fn config() -> MintConfig {
    serde_json::from_value(json!({
        "github_audience": AUDIENCE,
        "signing_keys": [],
        "ttl": "2h",
        "rules": [
            { "repository_id": REPO_ID, "event_name": ["push"], "ref": ["refs/heads/develop"], "scope": "read_write" },
            { "repository_id": REPO_ID, "scope": "read" },
        ],
    }))
    .unwrap()
}

async fn minter(secrets: &[(&str, &[u8])]) -> Arc<Minter> {
    Arc::new(
        Minter::from_jwks_text(config(), JWKS, secrets)
            .await
            .unwrap(),
    )
}

struct Harness {
    app: App,
    store: Arc<MemoryStore>,
}

fn harness(minter: Arc<Minter>) -> Harness {
    let store = Arc::new(MemoryStore::new());
    let providers: Vec<Arc<dyn Authenticator>> = vec![minter.clone()];
    Harness {
        app: app(AppState {
            store: store.clone(),
            auth: AuthChain::new(providers),
            max_artifact_bytes: 1024,
            minter: Some(minter),
        }),
        store,
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn github_token(event: &str, git_ref: &str, repo_id: u64, aud: &str) -> String {
    let claims = json!({
        "iss": "https://token.actions.githubusercontent.com",
        "aud": aud,
        "sub": format!("repo:graycoreio/daffodil:{event}"),
        "repository_id": repo_id.to_string(),
        "repository": "graycoreio/daffodil",
        "event_name": event,
        "ref": git_ref,
        "run_id": "987",
        "sha": "abc123",
        "iat": now(),
        "nbf": now() - 5,
        "exp": now() + 300,
    });
    let key = EncodingKey::from_rsa_pem(include_bytes!("fixtures/test_rsa_private.pem")).unwrap();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key-1".into());
    jsonwebtoken::encode(&header, &claims, &key).unwrap()
}

fn push_develop() -> String {
    github_token("push", "refs/heads/develop", REPO_ID, AUDIENCE)
}

fn pull_request() -> String {
    github_token("pull_request", "refs/pull/7/merge", REPO_ID, AUDIENCE)
}

impl Harness {
    async fn send(&self, method: Method, path: &str, token: &str, body: &[u8]) -> Response {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_LENGTH, body.len())
            .body(Body::from(body.to_vec()))
            .unwrap();
        self.app.clone().oneshot(req).await.unwrap()
    }

    async fn exchange(&self, github: &str) -> (StatusCode, Value) {
        let res = self
            .send(Method::POST, "/v1/token/github", github, b"")
            .await;
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }
}

#[tokio::test]
async fn push_to_develop_mints_a_writer_token() {
    let h = harness(minter(&[("k1", NEW_SECRET)]).await);
    let (status, body) = h.exchange(&push_develop()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["scope"], "read_write");
    assert_eq!(body["expires_in"], 7200);
    let token = body["token"].as_str().unwrap();

    assert_eq!(
        h.send(Method::PUT, "/v1/cache/1", token, b"x")
            .await
            .status(),
        StatusCode::OK
    );
    let meta = h.store.meta(&CacheKey::parse("1").unwrap()).await.unwrap();
    assert_eq!(meta["provider"], "github_mint");
    assert_eq!(meta["repository"], "graycoreio/daffodil");
    assert_eq!(meta["event"], "push");
    assert_eq!(meta["run_id"], "987");
}

#[tokio::test]
async fn pull_request_mints_a_reader_token() {
    let h = harness(minter(&[("k1", NEW_SECRET)]).await);
    let (status, body) = h.exchange(&pull_request()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["scope"], "read");
    let token = body["token"].as_str().unwrap();
    assert_eq!(
        h.send(Method::GET, "/v1/cache/1", token, b"")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.send(Method::PUT, "/v1/cache/1", token, b"x")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn exchange_rejections() {
    let h = harness(minter(&[("k1", NEW_SECRET)]).await);
    let other_repo = github_token("push", "refs/heads/develop", REPO_ID + 1, AUDIENCE);
    assert_eq!(h.exchange(&other_repo).await.0, StatusCode::FORBIDDEN);
    let wrong_aud = github_token(
        "push",
        "refs/heads/develop",
        REPO_ID,
        "https://elsewhere.example",
    );
    assert_eq!(h.exchange(&wrong_aud).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(h.exchange("not-a-jwt").await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn github_token_cannot_be_used_directly_on_the_cache() {
    let h = harness(minter(&[("k1", NEW_SECRET)]).await);
    let res = h
        .send(Method::PUT, "/v1/cache/1", &push_develop(), b"x")
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn minted_token_cannot_be_used_for_another_exchange() {
    let h = harness(minter(&[("k1", NEW_SECRET)]).await);
    let (_, body) = h.exchange(&pull_request()).await;
    let minted = body["token"].as_str().unwrap();
    assert_eq!(h.exchange(minted).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn tampered_scope_is_rejected() {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let h = harness(minter(&[("k1", NEW_SECRET)]).await);
    let (_, body) = h.exchange(&pull_request()).await;
    let token = body["token"].as_str().unwrap();
    let parts: Vec<&str> = token.split('.').collect();
    let mut claims: Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    claims["scope"] = json!("read_write");
    let forged = format!(
        "{}.{}.{}",
        parts[0],
        URL_SAFE_NO_PAD.encode(claims.to_string()),
        parts[2]
    );
    assert_eq!(
        h.send(Method::PUT, "/v1/cache/1", &forged, b"x")
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn key_rotation() {
    // Tokens minted before rotation (old key) keep working while the old key is still listed.
    let before = harness(minter(&[("old", OLD_SECRET)]).await);
    let (_, body) = before.exchange(&push_develop()).await;
    let old_token = body["token"].as_str().unwrap().to_owned();

    let after = harness(minter(&[("new", NEW_SECRET), ("old", OLD_SECRET)]).await);
    assert_eq!(
        after
            .send(Method::PUT, "/v1/cache/1", &old_token, b"x")
            .await
            .status(),
        StatusCode::OK
    );

    // New tokens are signed with the first key.
    let (_, body) = after.exchange(&push_develop()).await;
    let new_token = body["token"].as_str().unwrap();
    let kid = jsonwebtoken::decode_header(new_token).unwrap().kid.unwrap();
    assert_eq!(kid, "new");

    // Once the old key is removed, old tokens stop working.
    let removed = harness(minter(&[("new", NEW_SECRET)]).await);
    assert_eq!(
        removed
            .send(Method::PUT, "/v1/cache/2", &old_token, b"x")
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn expired_minted_token_is_401() {
    let h = harness(minter(&[("k1", NEW_SECRET)]).await);
    let claims = json!({
        "iss": "nx-remote-cache", "aud": "nx-remote-cache", "sub": "x", "scope": "read_write",
        "iat": now() - 7300, "nbf": now() - 7300, "exp": now() - 100,
    });
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("k1".into());
    let token =
        jsonwebtoken::encode(&header, &claims, &EncodingKey::from_secret(NEW_SECRET)).unwrap();
    for method in [Method::GET, Method::PUT] {
        let status = h
            .send(method.clone(), "/v1/cache/1", &token, b"x")
            .await
            .status();
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method}");
    }
}

#[tokio::test]
async fn short_secrets_are_refused() {
    let err = Minter::from_jwks_text(config(), JWKS, &[("k1", b"too-short")]).await;
    assert!(err.is_err());
}
