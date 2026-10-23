//! `GET`/`PUT /v1/cache/{hash}`, per the Nx remote cache OpenAPI spec.

use std::sync::Arc;

use axum::{
    body::{Body, BodyDataStream},
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::{StreamExt, stream};
use tokio::sync::Mutex;

use super::{AppState, authenticate};
use crate::{
    auth::Scope,
    error::ApiError,
    store::{CacheKey, PutOutcome, StoreError},
};

fn parse_key(hash: &str) -> Result<CacheKey, ApiError> {
    CacheKey::parse(hash).ok_or_else(|| ApiError::BadRequest("invalid cache hash".into()))
}

pub async fn get(
    State(state): State<AppState>,
    Path(hash): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // Every principal has at least Read, so there is no 403 on GET (Nx treats one as an error).
    authenticate(&state, &headers).await?;
    let key = parse_key(&hash)?;
    let artifact = state
        .store
        .get(&key)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, hash, "cache read failed");
            ApiError::Internal
        })?
        .ok_or(ApiError::NotFound)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
            (header::CONTENT_LENGTH, artifact.len.to_string()),
        ],
        Body::from_stream(artifact.body),
    )
        .into_response())
}

pub async fn put(
    State(state): State<AppState>,
    Path(hash): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<StatusCode, ApiError> {
    let principal = authenticate(&state, &headers).await?;
    let key = parse_key(&hash)?;
    let len: u64 = headers
        .get(header::CONTENT_LENGTH)
        .ok_or(ApiError::LengthRequired)?
        .to_str()
        .ok()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ApiError::BadRequest("invalid Content-Length".into()))?;
    if len > state.max_artifact_bytes {
        return Err(ApiError::PayloadTooLarge(state.max_artifact_bytes));
    }

    // From here on every answer waits for the whole body (at most max_artifact_bytes, checked
    // above): hyper closes the connection on a body left unread, and Nx, which sends the whole
    // body before reading the response, then fails the task on the broken pipe instead of
    // seeing the status. A 403 it would skip quietly becomes a failed run.
    let body = Arc::new(Mutex::new(body.into_data_stream()));
    if !principal.scope.allows(Scope::ReadWrite) {
        drain(&body).await;
        // Nx treats 403 on PUT as "not allowed to store" and silently skips the upload.
        return Err(ApiError::Forbidden("this token is read-only".into()));
    }

    let result = state
        .store
        .put_if_absent(&key, len, shared(body.clone()), &principal.meta)
        .await;
    // The store may stop reading early (e.g. storage refusing the first block).
    if !matches!(result, Err(StoreError::Body(_))) {
        drain(&body).await;
    }
    match result {
        Ok(PutOutcome::Created) => {
            tracing::info!(hash, len, writer = %principal.subject, provider = principal.provider, "stored artifact");
            Ok(StatusCode::OK)
        }
        Ok(PutOutcome::AlreadyExists) => Err(ApiError::Conflict),
        Err(StoreError::Body(e)) => Err(ApiError::BadRequest(format!("upload failed: {e}"))),
        // 403, not 500: Nx skips the upload on 403 instead of failing the run. This is how a
        // read-only identity behind a server configured as writable behaves.
        Err(StoreError::WriteDenied(e)) => {
            tracing::warn!(error = %e, hash, "storage refused the write");
            Err(ApiError::Forbidden(
                "this server's storage identity can't write to the cache".into(),
            ))
        }
        Err(e) => {
            tracing::error!(error = %e, hash, "cache write failed");
            Err(ApiError::Internal)
        }
    }
}

/// The upload body as a store stream that leaves the rest of the body readable for `drain`.
fn shared(body: Arc<Mutex<BodyDataStream>>) -> crate::store::ByteStream {
    Box::pin(stream::unfold(body, |body| async move {
        let chunk = body.lock().await.next().await?;
        Some((chunk.map_err(std::io::Error::other), body))
    }))
}

/// Reads and discards what is left of the upload body.
async fn drain(body: &Mutex<BodyDataStream>) {
    let mut body = body.lock().await;
    while let Some(Ok(_)) = body.next().await {}
}
