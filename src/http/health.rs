use axum::extract::State;

use super::AppState;
use crate::error::ApiError;

/// Liveness: the process is up. No dependencies, no auth.
pub async fn health() -> &'static str {
    "OK"
}

/// Readiness: the storage backend is reachable with our credentials.
pub async fn ready(State(state): State<AppState>) -> Result<&'static str, ApiError> {
    state.store.ready().await.map_err(|e| {
        tracing::warn!(error = %e, "readiness check failed");
        ApiError::Unavailable("storage backend unavailable".into())
    })?;
    Ok("OK")
}
