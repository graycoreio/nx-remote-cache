//! Conformance with the vendored Nx OpenAPI spec (`spec/nx-remote-cache.openapi.json`).
//!
//! If Nx changes the spec and it is re-vendored, this fails until the server (and the tests
//! named below) are updated to match.

use std::{collections::BTreeSet, sync::Arc};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use nx_remote_cache::{
    auth::{
        AuthChain, Authenticator, Scope,
        static_token::{StaticTokenEntry, StaticTokens, hash_token},
    },
    http::{AppState, app},
    store::memory::MemoryStore,
};
use serde_json::Value;
use tower::ServiceExt;

/// Every (path, method, status) the spec documents, and where it is exercised.
const COVERED: &[(&str, &str, &str, &str)] = &[
    (
        "/v1/cache/{hash}",
        "put",
        "200",
        "api::put_get_roundtrip_and_no_overwrite",
    ),
    (
        "/v1/cache/{hash}",
        "put",
        "401",
        "api::missing_token_is_401_text_plain",
    ),
    (
        "/v1/cache/{hash}",
        "put",
        "403",
        "api::read_only_token_gets_403_on_put",
    ),
    (
        "/v1/cache/{hash}",
        "put",
        "409",
        "api::put_get_roundtrip_and_no_overwrite",
    ),
    (
        "/v1/cache/{hash}",
        "get",
        "200",
        "api::put_get_roundtrip_and_no_overwrite",
    ),
    // Unreachable by design: every principal has Read, and Nx fails the run on a GET 403.
    (
        "/v1/cache/{hash}",
        "get",
        "403",
        "(intentionally never returned)",
    ),
    (
        "/v1/cache/{hash}",
        "get",
        "404",
        "api::put_get_roundtrip_and_no_overwrite",
    ),
];

fn spec() -> Value {
    serde_json::from_str(include_str!("../spec/nx-remote-cache.openapi.json")).unwrap()
}

#[test]
fn every_documented_response_is_covered() {
    let spec = spec();
    let mut documented = BTreeSet::new();
    for (path, ops) in spec["paths"].as_object().unwrap() {
        for (method, op) in ops.as_object().unwrap() {
            for status in op["responses"].as_object().unwrap().keys() {
                documented.insert((path.clone(), method.clone(), status.clone()));
            }
        }
    }
    let covered: BTreeSet<_> = COVERED
        .iter()
        .map(|(p, m, s, _)| (p.to_string(), m.to_string(), s.to_string()))
        .collect();
    assert_eq!(
        documented, covered,
        "spec changed: update the server and COVERED"
    );
}

#[test]
fn text_plain_error_bodies_match_spec() {
    // The spec declares text/plain for 401 and 403; ApiError always uses text/plain.
    let spec = spec();
    let put = &spec["paths"]["/v1/cache/{hash}"]["put"]["responses"];
    for status in ["401", "403"] {
        assert!(put[status]["content"]["text/plain"].is_object(), "{status}");
    }
}

#[tokio::test]
async fn every_documented_operation_is_routed() {
    let providers: Vec<Arc<dyn Authenticator>> = vec![Arc::new(
        StaticTokens::new(vec![StaticTokenEntry {
            name: "t".into(),
            sha256: hash_token("t"),
            scope: Scope::ReadWrite,
        }])
        .unwrap(),
    )];
    let app = app(AppState {
        store: Arc::new(MemoryStore::new()),
        auth: AuthChain::new(providers),
        max_artifact_bytes: 1024,
    });
    let spec = spec();
    for (path, ops) in spec["paths"].as_object().unwrap() {
        for method in ops.as_object().unwrap().keys() {
            let uri = path.replace("{hash}", "123");
            let req = Request::builder()
                .method(Method::from_bytes(method.to_uppercase().as_bytes()).unwrap())
                .uri(&uri)
                .header(header::AUTHORIZATION, "Bearer t")
                .header(header::CONTENT_LENGTH, 1)
                .body(Body::from("x"))
                .unwrap();
            let status = app.clone().oneshot(req).await.unwrap().status();
            assert!(
                status != StatusCode::METHOD_NOT_ALLOWED && status.as_u16() < 500,
                "{method} {path} is not served (got {status})"
            );
        }
    }
}
