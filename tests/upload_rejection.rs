//! Rejected uploads over a real connection. Nx writes the whole body before it reads the
//! response, so a server that answers early and closes the connection makes the upload fail
//! with a broken pipe, and Nx then fails the task instead of skipping the upload as it does
//! for a clean 403.

use std::sync::Arc;

use async_trait::async_trait;
use futures::StreamExt;
use nx_remote_cache::{
    auth::{
        AuthChain, Scope,
        static_token::{StaticTokenEntry, StaticTokens, hash_token},
    },
    http::{AppState, app},
    store::{
        Artifact, ByteStream, CacheKey, CacheStore, PutOutcome, StoreError, memory::MemoryStore,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tower::make::Shared;

const READ_TOKEN: &str = "read-token";
const WRITE_TOKEN: &str = "write-token";
/// Large enough that the socket buffers can't absorb the body unread.
const BODY: usize = 32 * 1024 * 1024;

/// Accepts the first chunk, then refuses the write, like Azure storage with a reader identity.
struct DenyingStore;

#[async_trait]
impl CacheStore for DenyingStore {
    async fn get(&self, _: &CacheKey) -> Result<Option<Artifact>, StoreError> {
        Ok(None)
    }
    async fn put_if_absent(
        &self,
        _: &CacheKey,
        _: u64,
        mut body: ByteStream,
        _: &str,
    ) -> Result<PutOutcome, StoreError> {
        body.next().await;
        Err(StoreError::WriteDenied(
            "AuthorizationPermissionMismatch".into(),
        ))
    }
    async fn ready(&self) -> Result<(), StoreError> {
        Ok(())
    }
}

async fn serve(store: Arc<dyn CacheStore>) -> std::net::SocketAddr {
    let auth = AuthChain::new(vec![Arc::new(
        StaticTokens::new(vec![
            StaticTokenEntry {
                name: "reader".into(),
                sha256: hash_token(READ_TOKEN),
                scope: Scope::Read,
            },
            StaticTokenEntry {
                name: "writer".into(),
                sha256: hash_token(WRITE_TOKEN),
                scope: Scope::ReadWrite,
            },
        ])
        .unwrap(),
    )]);
    let state = AppState {
        store,
        auth,
        max_artifact_bytes: 2 * BODY as u64,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, Shared::new(app(state))).await });
    addr
}

/// Sends the whole body, then reads the status line. Errors if the server hangs up first.
async fn upload(addr: std::net::SocketAddr, token: &str) -> std::io::Result<String> {
    let mut conn = TcpStream::connect(addr).await?;
    let head = format!(
        "PUT /v1/cache/123 HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/octet-stream\r\nContent-Length: {BODY}\r\n\r\n"
    );
    conn.write_all(head.as_bytes()).await?;
    let chunk = vec![b'x'; 64 * 1024];
    for _ in 0..BODY / chunk.len() {
        conn.write_all(&chunk).await?;
    }
    let mut response = vec![0; 1024];
    let n = conn.read(&mut response).await?;
    let response = String::from_utf8_lossy(&response[..n]);
    Ok(response.lines().next().unwrap_or_default().to_owned())
}

#[tokio::test]
async fn read_only_token_gets_a_clean_403_for_a_large_upload() {
    let addr = serve(Arc::new(MemoryStore::new())).await;
    let status = upload(addr, READ_TOKEN)
        .await
        .expect("upload must not be cut off");
    assert_eq!(status, "HTTP/1.1 403 Forbidden");
}

#[tokio::test]
async fn storage_refusing_the_write_gets_a_clean_403_for_a_large_upload() {
    let addr = serve(Arc::new(DenyingStore)).await;
    let status = upload(addr, WRITE_TOKEN)
        .await
        .expect("upload must not be cut off");
    assert_eq!(status, "HTTP/1.1 403 Forbidden");
}

#[tokio::test]
async fn large_upload_is_stored() {
    let addr = serve(Arc::new(MemoryStore::new())).await;
    assert_eq!(upload(addr, WRITE_TOKEN).await.unwrap(), "HTTP/1.1 200 OK");
}
