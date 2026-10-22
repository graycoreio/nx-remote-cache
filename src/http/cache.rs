//! `GET`/`PUT /v1/cache/{hash}`, per the Nx remote cache OpenAPI spec.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures::TryStreamExt;

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
    if !principal.scope.allows(Scope::ReadWrite) {
        // Nx treats 403 on PUT as "not allowed to store" and silently skips the upload.
        return Err(ApiError::Forbidden("this token is read-only".into()));
    }
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

    let stream = Box::pin(body.into_data_stream().map_err(std::io::Error::other));
    match state
        .store
        .put_if_absent(&key, len, stream, &principal.meta)
        .await
    {
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
