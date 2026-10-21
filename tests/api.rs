//! HTTP-level tests against the in-memory store: the Nx spec's status codes, auth behaviour
//! and the Entra provider with locally signed tokens.

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
        AuthChain, Authenticator, Scope,
        anonymous::Anonymous,
        entra::{EntraAuthenticator, EntraConfig},
        static_token::{StaticTokenEntry, StaticTokens, hash_token},
    },
    http::{App, AppState, app},
    store::{CacheKey, memory::MemoryStore},
};
use serde_json::json;
use tower::ServiceExt;

const READ_TOKEN: &str = "read-token";
const WRITE_TOKEN: &str = "write-token";
const TENANT: &str = "11111111-1111-1111-1111-111111111111";
const API_CLIENT_ID: &str = "22222222-2222-2222-2222-222222222222";
const MAX: u64 = 1024;

struct Harness {
    app: App,
    store: Arc<MemoryStore>,
}

async fn harness(anonymous: bool) -> Harness {
    let store = Arc::new(MemoryStore::new());
    let mut providers: Vec<Arc<dyn Authenticator>> = vec![
        Arc::new(entra().await),
        Arc::new(
            StaticTokens::new(vec![
                StaticTokenEntry {
                    name: "dev".into(),
                    sha256: hash_token(READ_TOKEN),
                    scope: Scope::Read,
                },
                StaticTokenEntry {
                    name: "ci".into(),
                    sha256: hash_token(WRITE_TOKEN),
                    scope: Scope::ReadWrite,
                },
            ])
            .unwrap(),
        ),
    ];
    if anonymous {
        providers.push(Arc::new(Anonymous { scope: Scope::Read }));
    }
    let state = AppState {
        store: store.clone(),
        auth: AuthChain::new(providers),
        max_artifact_bytes: MAX,
        #[cfg(feature = "mint")]
        minter: None,
    };
    Harness {
        app: app(state),
        store,
    }
}

fn entra_config() -> EntraConfig {
    serde_json::from_value(json!({
        "tenant_id": TENANT,
        "audiences": [API_CLIENT_ID],
    }))
    .unwrap()
}

async fn entra() -> EntraAuthenticator {
    let jwks = include_str!("fixtures/test_jwks.json");
    EntraAuthenticator::from_jwks_text(entra_config(), jwks)
        .await
        .unwrap()
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn entra_claims(roles: &[&str]) -> serde_json::Value {
    json!({
        "iss": entra_config().issuer(),
        "aud": API_CLIENT_ID,
        "tid": TENANT,
        "sub": "writer-sp-object-id",
        "oid": "writer-sp-object-id",
        "azp": "33333333-3333-3333-3333-333333333333",
        "roles": roles,
        "iat": now(),
        "nbf": now() - 10,
        "exp": now() + 3600,
    })
}

fn sign(claims: &serde_json::Value) -> String {
    let key = EncodingKey::from_rsa_pem(include_bytes!("fixtures/test_rsa_private.pem")).unwrap();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key-1".into());
    jsonwebtoken::encode(&header, claims, &key).unwrap()
}

impl Harness {
    async fn send(&self, req: Request<Body>) -> Response {
        self.app.clone().oneshot(req).await.unwrap()
    }

    async fn get(&self, path: &str, token: Option<&str>) -> Response {
        let mut req = Request::builder().method(Method::GET).uri(path);
        if let Some(token) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        self.send(req.body(Body::empty()).unwrap()).await
    }

    async fn put(&self, path: &str, token: Option<&str>, body: &[u8]) -> Response {
        self.put_with_len(path, token, body, Some(body.len() as u64))
            .await
    }

    async fn put_with_len(
        &self,
        path: &str,
        token: Option<&str>,
        body: &[u8],
        len: Option<u64>,
    ) -> Response {
        let mut req = Request::builder()
            .method(Method::PUT)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/octet-stream");
        if let Some(token) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if let Some(len) = len {
            req = req.header(header::CONTENT_LENGTH, len);
        }
        self.send(req.body(Body::from(body.to_vec())).unwrap())
            .await
    }
}

async fn text(res: Response) -> String {
    String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
}

fn assert_text_plain(res: &Response) {
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/plain");
}

#[tokio::test]
async fn missing_token_is_401_text_plain() {
    let h = harness(false).await;
    let res = h.get("/v1/cache/123", None).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    // Nx reports a 401 without exactly `text/plain` as a misconfigured server.
    assert_text_plain(&res);
    assert_eq!(res.headers()[header::WWW_AUTHENTICATE], "Bearer");
}

#[tokio::test]
async fn unknown_and_malformed_tokens_are_401() {
    let h = harness(false).await;
    assert_eq!(
        h.get("/v1/cache/123", Some("nope")).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let req = Request::get("/v1/cache/123")
        .header(header::AUTHORIZATION, "Basic dXNlcjpwYXNz")
        .body(Body::empty())
        .unwrap();
    assert_eq!(h.send(req).await.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn put_get_roundtrip_and_no_overwrite() {
    let h = harness(false).await;
    assert_eq!(
        h.get("/v1/cache/123", Some(WRITE_TOKEN)).await.status(),
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        h.put("/v1/cache/123", Some(WRITE_TOKEN), b"first")
            .await
            .status(),
        StatusCode::OK
    );
    let conflict = h.put("/v1/cache/123", Some(WRITE_TOKEN), b"second").await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    let res = h.get("/v1/cache/123", Some(READ_TOKEN)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()[header::CONTENT_TYPE],
        "application/octet-stream"
    );
    assert_eq!(res.headers()[header::CONTENT_LENGTH], "5");
    assert_eq!(text(res).await, "first");
}

#[tokio::test]
async fn read_only_token_gets_403_on_put() {
    let h = harness(false).await;
    let res = h.put("/v1/cache/123", Some(READ_TOKEN), b"data").await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_text_plain(&res);
    assert!(
        h.store
            .meta(&CacheKey::parse("123").unwrap())
            .await
            .is_none()
    );
}

#[tokio::test]
async fn put_validation() {
    let h = harness(false).await;
    let missing_len = h
        .put_with_len("/v1/cache/1", Some(WRITE_TOKEN), b"data", None)
        .await;
    assert_eq!(missing_len.status(), StatusCode::LENGTH_REQUIRED);

    let too_big = vec![0u8; MAX as usize + 1];
    assert_eq!(
        h.put("/v1/cache/2", Some(WRITE_TOKEN), &too_big)
            .await
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );

    let short = h
        .put_with_len("/v1/cache/3", Some(WRITE_TOKEN), b"data", Some(10))
        .await;
    assert_eq!(short.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        h.get("/v1/cache/3", Some(WRITE_TOKEN)).await.status(),
        StatusCode::NOT_FOUND
    );

    let bad_hash = h.put("/v1/cache/a.b", Some(WRITE_TOKEN), b"data").await;
    assert_eq!(bad_hash.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn invalid_hash_is_not_revealed_to_unauthenticated_callers() {
    let h = harness(false).await;
    assert_eq!(
        h.get("/v1/cache/a.b", None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn double_slash_from_trailing_slash_server_url_is_tolerated() {
    let h = harness(false).await;
    assert_eq!(
        h.put("//v1/cache/42", Some(WRITE_TOKEN), b"x")
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        h.get("//v1/cache/42", Some(READ_TOKEN)).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn concurrent_puts_have_exactly_one_winner() {
    let h = Arc::new(harness(false).await);
    let tasks: Vec<_> = (0..16)
        .map(|i| {
            let h = h.clone();
            tokio::spawn(async move {
                h.put(
                    "/v1/cache/race",
                    Some(WRITE_TOKEN),
                    format!("writer-{i}").as_bytes(),
                )
                .await
                .status()
            })
        })
        .collect();
    let mut statuses = Vec::new();
    for t in tasks {
        statuses.push(t.await.unwrap());
    }
    assert_eq!(statuses.iter().filter(|s| **s == StatusCode::OK).count(), 1);
    assert_eq!(
        statuses
            .iter()
            .filter(|s| **s == StatusCode::CONFLICT)
            .count(),
        15
    );
}

#[tokio::test]
async fn anonymous_read_when_enabled() {
    let h = harness(true).await;
    h.put("/v1/cache/7", Some(WRITE_TOKEN), b"x").await;
    assert_eq!(h.get("/v1/cache/7", None).await.status(), StatusCode::OK);
    assert_eq!(
        h.put("/v1/cache/8", None, b"x").await.status(),
        StatusCode::FORBIDDEN
    );
    // A bad token is still rejected rather than silently downgraded to anonymous.
    assert_eq!(
        h.get("/v1/cache/7", Some("nope")).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn health_and_ready() {
    let h = harness(false).await;
    assert_eq!(h.get("/health", None).await.status(), StatusCode::OK);
    assert_eq!(h.get("/ready", None).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn entra_write_role_can_put_and_records_provenance() {
    let h = harness(false).await;
    let token = sign(&entra_claims(&["Cache.Write"]));
    assert_eq!(
        h.put("/v1/cache/9", Some(&token), b"x").await.status(),
        StatusCode::OK
    );
    let meta = h.store.meta(&CacheKey::parse("9").unwrap()).await.unwrap();
    assert_eq!(meta["provider"], "entra");
    assert_eq!(meta["subject"], "writer-sp-object-id");
    assert_eq!(meta["client_id"], "33333333-3333-3333-3333-333333333333");
}

#[tokio::test]
async fn entra_read_role_cannot_put() {
    let h = harness(false).await;
    let token = sign(&entra_claims(&["Cache.Read"]));
    assert_eq!(
        h.get("/v1/cache/9", Some(&token)).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.put("/v1/cache/9", Some(&token), b"x").await.status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn entra_rejects_bad_tokens() {
    let h = harness(false).await;
    let mut cases: Vec<(&str, serde_json::Value)> = Vec::new();

    cases.push(("no roles", entra_claims(&[])));
    cases.push(("unrelated role", entra_claims(&["Something.Else"])));

    let mut c = entra_claims(&["Cache.Write"]);
    c["aud"] = json!("api://some-other-api");
    cases.push(("wrong audience", c));

    let mut c = entra_claims(&["Cache.Write"]);
    c["nbf"] = json!(now() + 3600);
    cases.push(("not yet valid", c));

    let mut c = entra_claims(&["Cache.Write"]);
    c["tid"] = json!("99999999-9999-9999-9999-999999999999");
    cases.push(("wrong tenant id", c));

    let mut c = entra_claims(&["Cache.Write"]);
    c["iss"] = json!("https://login.microsoftonline.com/99999999-9999-9999-9999-999999999999/v2.0");
    cases.push(("another tenant's issuer", c));

    let mut c = entra_claims(&["Cache.Write"]);
    c["iss"] = json!("https://token.actions.githubusercontent.com");
    cases.push(("github issuer without the mint provider", c));

    for (name, claims) in cases {
        let res = h.put("/v1/cache/10", Some(&sign(&claims)), b"x").await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{name}");
        assert_text_plain(&res);
    }
    assert!(
        h.store
            .meta(&CacheKey::parse("10").unwrap())
            .await
            .is_none()
    );
}

#[tokio::test]
async fn entra_expired_token_is_401_like_any_invalid_token() {
    // Expiry gets no special treatment: a 401 makes Nx fail the run, which surfaces stale
    // CI credentials instead of silently losing cache reads and writes.
    let h = harness(false).await;
    h.put("/v1/cache/13", Some(WRITE_TOKEN), b"x").await;
    let mut claims = entra_claims(&["Cache.Write"]);
    claims["exp"] = json!(now() - 3600);
    let token = sign(&claims);

    let get = h.get("/v1/cache/13", Some(&token)).await;
    assert_eq!(get.status(), StatusCode::UNAUTHORIZED);
    assert_text_plain(&get);
    let put = h.put("/v1/cache/14", Some(&token), b"x").await;
    assert_eq!(put.status(), StatusCode::UNAUTHORIZED);
    assert!(
        h.store
            .meta(&CacheKey::parse("14").unwrap())
            .await
            .is_none()
    );
}

#[tokio::test]
async fn entra_rejects_tampered_signature() {
    let h = harness(false).await;
    let token = sign(&entra_claims(&["Cache.Read"]));
    // Swap in a payload granting write, keeping the original signature.
    let forged_payload = {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        URL_SAFE_NO_PAD.encode(entra_claims(&["Cache.Write"]).to_string())
    };
    let mut parts: Vec<&str> = token.split('.').collect();
    parts[1] = &forged_payload;
    let forged = parts.join(".");
    assert_eq!(
        h.put("/v1/cache/11", Some(&forged), b"x").await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn entra_rejects_hs256_signed_with_public_material() {
    // Classic alg-confusion attempt: an HMAC token "signed" with something public. The RSA key
    // only allows RSA algorithms, so this must fail regardless of the secret used.
    let h = harness(false).await;
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("test-key-1".into());
    let jwks = include_str!("fixtures/test_jwks.json");
    let token = jsonwebtoken::encode(
        &header,
        &entra_claims(&["Cache.Write"]),
        &EncodingKey::from_secret(jwks.as_bytes()),
    )
    .unwrap();
    assert_eq!(
        h.put("/v1/cache/12", Some(&token), b"x").await.status(),
        StatusCode::UNAUTHORIZED
    );
}
