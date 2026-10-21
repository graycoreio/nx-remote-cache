//! `POST /v1/token/github` (feature `mint`; not part of the Nx spec).
//! `Authorization: Bearer <GitHub Actions OIDC token>` → `{ token, scope, expires_in }`.

use axum::{Json, extract::State, http::HeaderMap};

use super::{AppState, bearer};
use crate::{
    auth::{AuthError, mint::MintedToken},
    error::ApiError,
};

pub async fn github(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<MintedToken>, ApiError> {
    let minter = state.minter.as_ref().ok_or(ApiError::NotFound)?;
    let token = bearer(&headers)?
        .ok_or_else(|| ApiError::Unauthorized("missing GitHub OIDC token".into()))?;
    match minter.exchange(token).await {
        Ok(minted) => {
            tracing::info!(scope = ?minted.scope, "minted token");
            Ok(Json(minted))
        }
        Err(AuthError::NoAccess) => Err(ApiError::Forbidden(
            "no rule grants this workflow access".into(),
        )),
        Err(e) => {
            tracing::info!(error = %e, "token exchange rejected");
            Err(ApiError::Unauthorized("invalid GitHub OIDC token".into()))
        }
    }
}
