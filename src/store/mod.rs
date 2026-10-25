//! Storage abstraction. Handlers only ever talk to [`CacheStore`]; nothing above this module
//! knows which backend is in use.

pub mod azure;
pub mod memory;

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, stream::BoxStream};

pub type ByteStream = BoxStream<'static, Result<Bytes, io::Error>>;

/// A validated Nx task hash. Restricting the alphabet means a key can never be used to
/// address anything outside the cache prefix.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

impl CacheKey {
    pub const MAX_LEN: usize = 128;

    pub fn parse(raw: &str) -> Option<Self> {
        let valid = !raw.is_empty()
            && raw.len() <= Self::MAX_LEN
            && raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        valid.then(|| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct Artifact {
    pub len: u64,
    pub body: ByteStream,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Created,
    AlreadyExists,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The client sent fewer or more bytes than it declared, or the connection dropped.
    #[error("request body error: {0}")]
    Body(io::Error),
    /// The storage credential may read but not write (e.g. a reader identity's role, or a
    /// read-only SAS). The caller's request is fine; this identity just can't write.
    #[error("storage refused the write: {0}")]
    WriteDenied(String),
    #[error("storage backend error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait CacheStore: Send + Sync + 'static {
    async fn get(&self, key: &CacheKey) -> Result<Option<Artifact>, StoreError>;

    /// Atomically create the artifact if, and only if, it does not exist yet. The no-overwrite
    /// guarantee (the CREEP mitigation the Nx spec requires via 409) lives here, not in the
    /// handler, because a separate exists-then-put check would race.
    async fn put_if_absent(
        &self,
        key: &CacheKey,
        len: u64,
        body: ByteStream,
        // Who wrote it (the token's name), recorded with the artifact.
        writer: &str,
    ) -> Result<PutOutcome, StoreError>;

    /// Cheap check that the backend is reachable and we are authorized. Run once at startup,
    /// before the server listens.
    async fn ready(&self) -> Result<(), StoreError>;
}

/// Wraps an upload body so that it yields exactly `expected` bytes, erroring otherwise.
pub fn exact_length(body: ByteStream, expected: u64) -> ByteStream {
    Box::pin(ExactLength {
        inner: body,
        expected,
        seen: 0,
        done: false,
    })
}

struct ExactLength {
    inner: ByteStream,
    expected: u64,
    seen: u64,
    done: bool,
}

impl Stream for ExactLength {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.seen += chunk.len() as u64;
                if self.seen > self.expected {
                    self.done = true;
                    return Poll::Ready(Some(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "body is longer than Content-Length",
                    ))));
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => {
                self.done = true;
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                self.done = true;
                if self.seen < self.expected {
                    return Poll::Ready(Some(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "body is shorter than Content-Length",
                    ))));
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{StreamExt, TryStreamExt, stream};

    fn body(chunks: &[&'static [u8]]) -> ByteStream {
        let chunks: Vec<_> = chunks.iter().map(|c| Ok(Bytes::from_static(c))).collect();
        stream::iter(chunks).boxed()
    }

    #[test]
    fn cache_key_rejects_path_tricks() {
        assert!(CacheKey::parse("1234567890").is_some());
        assert!(CacheKey::parse("abc_DEF-123").is_some());
        for bad in ["", "../x", "a/b", "a.b", "a%2Fb", "a b", &"x".repeat(129)] {
            assert!(CacheKey::parse(bad).is_none(), "{bad:?} should be rejected");
        }
    }

    #[tokio::test]
    async fn exact_length_accepts_matching_body() {
        let out: Vec<Bytes> = exact_length(body(&[b"ab", b"cd"]), 4)
            .try_collect()
            .await
            .unwrap();
        assert_eq!(out.concat(), b"abcd");
    }

    #[tokio::test]
    async fn exact_length_rejects_short_and_long_bodies() {
        let short: Result<Vec<Bytes>, _> = exact_length(body(&[b"ab"]), 4).try_collect().await;
        assert_eq!(short.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        let long: Result<Vec<Bytes>, _> = exact_length(body(&[b"abcde"]), 4).try_collect().await;
        assert_eq!(long.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
