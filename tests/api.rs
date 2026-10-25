//! HTTP-level tests against the in-memory store: the Nx spec's status codes and auth behaviour.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use nx_remote_cache::{
    auth::{
        AuthChain, Scope,
        static_token::{StaticTokenEntry, StaticTokens, hash_token},
    },
    http::{App, AppState, app},
    store::{CacheKey, memory::MemoryStore},
};
use tower::ServiceExt;

const READ_TOKEN: &str = "read-token";
const WRITE_TOKEN: &str = "write-token";
const MAX: u64 = 1024;

struct Harness {
    app: App,
    store: Arc<MemoryStore>,
}

async fn harness() -> Harness {
    let store = Arc::new(MemoryStore::new());
    let tokens = StaticTokens::new(vec![
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
    .unwrap();
    let state = AppState {
        store: store.clone(),
        auth: AuthChain::new(vec![Arc::new(tokens)]),
        max_artifact_bytes: MAX,
    };
    Harness {
        app: app(state),
        store,
    }
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
    let h = harness().await;
    let res = h.get("/v1/cache/123", None).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    // Nx reports a 401 without exactly `text/plain` as a misconfigured server.
    assert_text_plain(&res);
    assert_eq!(res.headers()[header::WWW_AUTHENTICATE], "Bearer");
}

#[tokio::test]
async fn unknown_and_malformed_tokens_are_401() {
    let h = harness().await;
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
    let h = harness().await;
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
    let h = harness().await;
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
    let h = harness().await;
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
    let h = harness().await;
    assert_eq!(
        h.get("/v1/cache/a.b", None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn double_slash_from_trailing_slash_server_url_is_tolerated() {
    let h = harness().await;
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
    let h = Arc::new(harness().await);
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
async fn health_and_ready() {
    let h = harness().await;
    assert_eq!(h.get("/health", None).await.status(), StatusCode::OK);
    assert_eq!(h.get("/ready", None).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn writes_record_the_token_name_as_provenance() {
    let h = harness().await;
    assert_eq!(
        h.put("/v1/cache/9", Some(WRITE_TOKEN), b"x").await.status(),
        StatusCode::OK
    );
    let meta = h.store.meta(&CacheKey::parse("9").unwrap()).await.unwrap();
    assert_eq!(meta.get("token").map(String::as_str), Some("ci"));
}
