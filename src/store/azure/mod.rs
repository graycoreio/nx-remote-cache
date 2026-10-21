//! Azure Blob Storage backend.
//!
//! Uploads stream the request body into staged blocks (bounded memory: one block at a time)
//! and commit them with `If-None-Match: *`, so creating a blob is atomic and never overwrites.

mod credential;

use std::{collections::HashMap, num::NonZero};

use async_trait::async_trait;
use azure_core::{error::ErrorKind, http::StatusCode};
use azure_storage_blob::{
    BlobContainerClient,
    models::{
        BlobClientDownloadOptions, BlockBlobClientCommitBlockListOptions,
        BlockBlobClientUploadOptions, BlockLookupList,
    },
};
use bytes::{Bytes, BytesMut};
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;
use url::Url;

use credential::ResolvedAuth;
pub use credential::StorageAuth;

use super::{
    Artifact, ByteStream, CacheKey, CacheStore, PutOutcome, StoreError, WriteMeta, exact_length,
};

/// Blob used only to probe access; it never needs to exist.
const READY_PROBE_BLOB: &str = "ready-probe";

const DOWNLOAD_PARTITION_SIZE: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AzureConfig {
    /// Storage account name. Ignored when `endpoint` is set.
    pub account: Option<String>,
    /// Full blob service URL, e.g. `http://127.0.0.1:10000/devstoreaccount1` for Azurite.
    pub endpoint: Option<Url>,
    pub container: String,
    /// Virtual directory inside the container that all cache entries live under.
    #[serde(default = "default_prefix")]
    pub prefix: String,
    /// Uploads at or below this size go up in one request; larger ones are staged in blocks of
    /// this size. Also the upper bound on per-upload memory.
    #[serde(default = "default_block_size")]
    pub block_size: NonZero<usize>,
    /// Concurrent range requests per download. Each download buffers at most
    /// `2 * download_parallelism` partitions of 4 MiB, so the default bounds it at ~32 MiB.
    /// (The SDK default scales with CPU count, which effectively buffers whole artifacts.)
    #[serde(default = "default_download_parallelism")]
    pub download_parallelism: NonZero<usize>,
    pub auth: StorageAuth,
}

fn default_prefix() -> String {
    "v1".into()
}

fn default_download_parallelism() -> NonZero<usize> {
    NonZero::new(4).unwrap()
}

fn default_block_size() -> NonZero<usize> {
    NonZero::new(8 * 1024 * 1024).unwrap()
}

pub struct AzureBlobStore {
    container: BlobContainerClient,
    prefix: String,
    block_size: NonZero<usize>,
    download_parallelism: NonZero<usize>,
    describe: String,
}

impl AzureBlobStore {
    pub fn new(config: &AzureConfig) -> Result<Self, String> {
        let mut url = match (&config.endpoint, &config.account) {
            (Some(endpoint), _) => endpoint.clone(),
            (None, Some(account)) => {
                Url::parse(&format!("https://{account}.blob.core.windows.net"))
                    .map_err(|e| format!("invalid storage account name {account:?}: {e}"))?
            }
            (None, None) => return Err("storage config needs `account` or `endpoint`".into()),
        };
        let not_base = format!("storage endpoint {url} cannot be a base URL");
        url.path_segments_mut()
            .map_err(|_| not_base)?
            .pop_if_empty()
            .push(&config.container);
        let describe = format!("{url} (auth mode `{}`)", config.auth.describe());

        let credential = match config.auth.resolve()? {
            ResolvedAuth::Credential(credential) => Some(credential),
            ResolvedAuth::Sas(token) => {
                url.set_query(Some(&token));
                None
            }
        };
        let container = BlobContainerClient::new(url, credential, None)
            .map_err(|e| format!("cannot create blob client for {describe}: {e}"))?;
        Ok(Self {
            container,
            prefix: config.prefix.trim_matches('/').to_owned(),
            block_size: config.block_size,
            download_parallelism: config.download_parallelism,
            describe,
        })
    }

    pub fn describe(&self) -> &str {
        &self.describe
    }

    fn blob_name(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{}/{key}", self.prefix)
        }
    }

    async fn upload_small(
        &self,
        name: &str,
        data: Bytes,
        metadata: HashMap<String, String>,
    ) -> azure_core::Result<()> {
        let options = BlockBlobClientUploadOptions {
            metadata: Some(metadata),
            // Make sure the SDK sends this as one Put Blob rather than re-chunking it.
            partition_size: NonZero::new(self.block_size.get() as u64),
            ..Default::default()
        }
        .if_not_exists();
        self.container
            .blob_client(name)
            .upload(data.into(), Some(options))
            .await
            .map(drop)
    }

    async fn upload_blocks(
        &self,
        name: &str,
        first: Bytes,
        mut rest: ByteStream,
        metadata: HashMap<String, String>,
    ) -> Result<azure_core::Result<()>, StoreError> {
        let client = self.container.blob_client(name).block_blob_client();
        let block_size = self.block_size.get();
        let mut block_ids: Vec<Vec<u8>> = Vec::new();
        let mut buf = BytesMut::from(first);

        loop {
            let next = rest.next().await.transpose().map_err(StoreError::Body)?;
            if let Some(chunk) = &next {
                buf.extend_from_slice(chunk);
            }
            // Stage full blocks; on end of body also stage whatever is left.
            while buf.len() >= block_size || (next.is_none() && !buf.is_empty()) {
                let take = buf.len().min(block_size);
                let block = buf.split_to(take).freeze();
                // All block ids in a blob must have the same length; UUIDs are 16 bytes.
                let id = uuid::Uuid::new_v4().as_bytes().to_vec();
                if let Err(e) = client
                    .stage_block(&id, block.len() as u64, block.into(), None)
                    .await
                {
                    return Ok(Err(e));
                }
                block_ids.push(id);
            }
            if next.is_none() {
                break;
            }
        }

        let list = BlockLookupList {
            latest: Some(block_ids),
            ..Default::default()
        };
        let options = BlockBlobClientCommitBlockListOptions {
            if_none_match: Some("*".into()),
            metadata: Some(metadata),
            ..Default::default()
        };
        let content = match list.try_into() {
            Ok(content) => content,
            Err(e) => return Ok(Err(e)),
        };
        Ok(client
            .commit_block_list(content, Some(options))
            .await
            .map(drop))
    }
}

#[async_trait]
impl CacheStore for AzureBlobStore {
    async fn get(&self, key: &CacheKey) -> Result<Option<Artifact>, StoreError> {
        let blob = self.container.blob_client(&self.blob_name(key.as_str()));
        let options = BlobClientDownloadOptions {
            parallel: Some(self.download_parallelism),
            partition_size: NonZero::new(DOWNLOAD_PARTITION_SIZE),
            ..Default::default()
        };
        match blob.download(Some(options)).await {
            Ok(result) => {
                // Partitioned downloads report the first partition's Content-Length; the total
                // size is in Content-Range when present.
                let len = result
                    .headers
                    .get_optional_str(&"content-range".into())
                    .and_then(total_from_content_range)
                    .or(result.properties.content_length)
                    .ok_or_else(|| StoreError::Backend("download response has no length".into()))?;
                let body = result.body.map_err(std::io::Error::other).boxed();
                Ok(Some(Artifact { len, body }))
            }
            Err(e) if error_code(&e) == Some("BlobNotFound") => Ok(None),
            Err(e) => Err(backend(e)),
        }
    }

    async fn put_if_absent(
        &self,
        key: &CacheKey,
        len: u64,
        body: ByteStream,
        meta: &WriteMeta,
    ) -> Result<PutOutcome, StoreError> {
        let name = self.blob_name(key.as_str());
        let metadata = to_blob_metadata(meta);
        let mut body = exact_length(body, len);

        let result = if len <= self.block_size.get() as u64 {
            let chunks: Vec<Bytes> = body.try_collect().await.map_err(StoreError::Body)?;
            self.upload_small(&name, Bytes::from(chunks.concat()), metadata)
                .await
        } else {
            let first = body
                .next()
                .await
                .transpose()
                .map_err(StoreError::Body)?
                .unwrap_or_default();
            self.upload_blocks(&name, first, body, metadata).await?
        };

        match result {
            Ok(()) => Ok(PutOutcome::Created),
            Err(e) if is_conflict(&e) => Ok(PutOutcome::AlreadyExists),
            Err(e) => {
                // A concurrent writer may have committed first, which can surface as an error
                // other than 409 (e.g. our staged blocks were discarded by their commit).
                // If the blob exists now, the outcome for this client is the same as a 409.
                match self.container.blob_client(&name).exists().await {
                    Ok(true) => Ok(PutOutcome::AlreadyExists),
                    _ => Err(backend(e)),
                }
            }
        }
    }

    async fn ready(&self) -> Result<(), StoreError> {
        // Probe a blob that never exists. BlobNotFound proves the credential works and the
        // container exists; ContainerNotFound, 403s and credential failures are the
        // misconfigurations worth reporting. (`BlobClient::exists()` is no use here: it also
        // maps ContainerNotFound to `false`.) Needs only blob read permission, so a
        // container-scoped SAS works.
        let probe = self
            .container
            .blob_client(&self.blob_name(READY_PROBE_BLOB));
        match probe.get_properties(None).await {
            Ok(_) => Ok(()),
            Err(e) if error_code(&e) == Some("BlobNotFound") => Ok(()),
            Err(e) => Err(StoreError::Backend(format!(
                "{}: {}",
                self.describe,
                explain(&e)
            ))),
        }
    }
}

fn error_code(e: &azure_core::Error) -> Option<&str> {
    match e.kind() {
        ErrorKind::HttpResponse { error_code, .. } => error_code.as_deref(),
        _ => None,
    }
}

fn is_conflict(e: &azure_core::Error) -> bool {
    matches!(
        e.http_status(),
        Some(StatusCode::Conflict | StatusCode::PreconditionFailed)
    ) || error_code(e) == Some("BlobAlreadyExists")
}

fn backend(e: azure_core::Error) -> StoreError {
    StoreError::Backend(explain(&e))
}

/// Turn the common Azure misconfigurations into something actionable.
fn explain(e: &azure_core::Error) -> String {
    let hint = match (e.http_status(), error_code(e)) {
        (_, Some("ContainerNotFound")) => "the container does not exist; create it first",
        (Some(StatusCode::Forbidden), Some("AuthorizationPermissionMismatch")) => {
            // With a role scoped to one container, Azure also answers a missing container
            // with this 403 rather than ContainerNotFound.
            "authenticated, but the identity lacks the `Storage Blob Data Contributor` role on \
             the container, or the container does not exist"
        }
        (Some(StatusCode::Forbidden), _) => {
            "access denied: check the identity's role assignment, or the SAS token's permissions and expiry"
        }
        _ if matches!(e.kind(), ErrorKind::Credential) => {
            "could not obtain an Azure credential for the configured auth mode"
        }
        _ => "",
    };
    if hint.is_empty() {
        e.to_string()
    } else {
        format!("{hint} ({e})")
    }
}

/// `bytes 0-4194303/10485760` → 10485760.
fn total_from_content_range(value: &str) -> Option<u64> {
    value.rsplit_once('/')?.1.trim().parse().ok()
}

/// Blob metadata names must be C# identifiers and values must be ASCII header values.
fn to_blob_metadata(meta: &WriteMeta) -> HashMap<String, String> {
    meta.iter()
        .map(|(k, v)| {
            let mut name: String = k
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
                name.insert(0, 'm');
            }
            let value: String = v
                .chars()
                .filter(|c| c.is_ascii_graphic() || *c == ' ')
                .take(256)
                .collect();
            (name, value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http_error(status: StatusCode, code: &str) -> azure_core::Error {
        ErrorKind::HttpResponse {
            status,
            error_code: Some(code.to_owned()),
            raw_response: None,
        }
        .into_error()
    }

    // Azurite enforces SAS but not RBAC, so the RBAC case can't be produced by the Azurite
    // tests; these pin every explanation branch with errors shaped like real Azure's.
    #[test]
    fn explains_rbac_denial_including_missing_container() {
        let msg = explain(&http_error(
            StatusCode::Forbidden,
            "AuthorizationPermissionMismatch",
        ));
        assert!(
            msg.contains("lacks the `Storage Blob Data Contributor` role"),
            "{msg}"
        );
        assert!(msg.contains("or the container does not exist"), "{msg}");
    }

    #[test]
    fn explains_other_denials_and_missing_container() {
        let sas = explain(&http_error(StatusCode::Forbidden, "AuthenticationFailed"));
        assert!(sas.starts_with("access denied"), "{sas}");
        let missing = explain(&http_error(StatusCode::NotFound, "ContainerNotFound"));
        assert!(
            missing.starts_with("the container does not exist"),
            "{missing}"
        );
        let credential = explain(&ErrorKind::Credential.into_error());
        assert!(
            credential.starts_with("could not obtain an Azure credential"),
            "{credential}"
        );
    }

    #[test]
    fn unexplained_errors_pass_through() {
        let e = http_error(StatusCode::InternalServerError, "InternalError");
        assert_eq!(explain(&e), e.to_string());
    }

    #[test]
    fn conflicts_are_recognised() {
        assert!(is_conflict(&http_error(
            StatusCode::Conflict,
            "BlobAlreadyExists"
        )));
        assert!(is_conflict(&http_error(
            StatusCode::PreconditionFailed,
            "ConditionNotMet"
        )));
        assert!(!is_conflict(&http_error(
            StatusCode::Forbidden,
            "AuthorizationPermissionMismatch"
        )));
    }

    #[test]
    fn content_range_total() {
        assert_eq!(
            total_from_content_range("bytes 0-4194303/10485760"),
            Some(10485760)
        );
        assert_eq!(total_from_content_range("bytes 0-0/*"), None);
    }

    #[test]
    fn metadata_is_sanitized() {
        let meta = WriteMeta::from([
            ("provider".into(), "entra".into()),
            ("run-id".into(), "123".into()),
            ("1st".into(), "naïve\nvalue".into()),
        ]);
        let out = to_blob_metadata(&meta);
        assert_eq!(out["provider"], "entra");
        assert_eq!(out["run_id"], "123");
        assert_eq!(out["m1st"], "navevalue");
    }
}
