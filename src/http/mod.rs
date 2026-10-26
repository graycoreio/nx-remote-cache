mod cache;
mod health;

use std::sync::Arc;

use axum::{
    Router,
    extract::Request,
    http::{HeaderMap, Uri, header},
    routing::get,
};
use tower::{
    Layer,
    util::{MapRequest, MapRequestLayer},
};
use tower_http::{catch_panic::CatchPanicLayer, trace::TraceLayer};

use crate::{
    auth::{AuthChain, AuthError, Principal},
    error::ApiError,
    store::CacheStore,
};

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn CacheStore>,
    pub auth: AuthChain,
    pub max_artifact_bytes: u64,
}

pub type App = MapRequest<Router, fn(Request) -> Request>;

/// The full service, including path normalization that has to happen before routing.
pub fn app(state: AppState) -> App {
    MapRequestLayer::new(collapse_slashes as fn(Request) -> Request).layer(router(state))
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/cache/{hash}", get(cache::get).put(cache::put))
        .route("/health", get(health::health))
        .layer(TraceLayer::new_for_http())
        .layer(CatchPanicLayer::new())
        .with_state(state)
}

/// Nx builds URLs as `format!("{server}/v1/cache/{hash}")`, so a server URL configured with a
/// trailing slash produces `//v1/cache/...`. Collapse repeated slashes instead of 404ing.
fn collapse_slashes(mut req: Request) -> Request {
    let path = req.uri().path();
    if path.contains("//") {
        let mut collapsed = String::with_capacity(path.len());
        for c in path.chars() {
            if !(c == '/' && collapsed.ends_with('/')) {
                collapsed.push(c);
            }
        }
        let mut parts = req.uri().clone().into_parts();
        let pq = match req.uri().query() {
            Some(q) => format!("{collapsed}?{q}"),
            None => collapsed,
        };
        if let Ok(pq) = pq.parse() {
            parts.path_and_query = Some(pq);
            if let Ok(uri) = Uri::from_parts(parts) {
                *req.uri_mut() = uri;
            }
        }
    }
    req
}

fn bearer(headers: &HeaderMap) -> Result<Option<&str>, ApiError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| ApiError::Unauthorized("malformed Authorization header".into()))?;
    match value.split_once(' ') {
        Some((scheme, token))
            if scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty() =>
        {
            Ok(Some(token.trim()))
        }
        _ => Err(ApiError::Unauthorized(
            "expected `Authorization: Bearer <token>`".into(),
        )),
    }
}

pub(crate) async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, ApiError> {
    let token = bearer(headers)?;
    // Every failure, expiry included, is a 401: Nx then fails the run, which is what should
    // happen when CI's cache credentials are wrong or stale.
    state.auth.authenticate(token).await.map_err(|e| {
        tracing::info!(error = %e, "authentication failed");
        let AuthError::Invalid(_) = e;
        ApiError::Unauthorized("missing or invalid authentication token".into())
    })
}
