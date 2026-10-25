//! In-process store for tests and local smoke runs. Not persistent.

use std::collections::{HashMap, hash_map::Entry};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{StreamExt, TryStreamExt, stream};
use tokio::sync::RwLock;

use super::{Artifact, CacheKey, CacheStore, PutOutcome, StoreError, UploadStream, exact_length};

#[derive(Default)]
pub struct MemoryStore {
    entries: RwLock<HashMap<CacheKey, (Bytes, String)>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The name of the token that wrote an entry.
    pub async fn writer(&self, key: &CacheKey) -> Option<String> {
        self.entries.read().await.get(key).map(|(_, w)| w.clone())
    }
}

#[async_trait]
impl CacheStore for MemoryStore {
    async fn get(&self, key: &CacheKey) -> Result<Option<Artifact>, StoreError> {
        Ok(self
            .entries
            .read()
            .await
            .get(key)
            .map(|(data, _)| Artifact {
                len: data.len() as u64,
                body: stream::once(std::future::ready(Ok(data.clone()))).boxed(),
            }))
    }

    async fn put_if_absent(
        &self,
        key: &CacheKey,
        len: u64,
        body: UploadStream<'_>,
        writer: &str,
    ) -> Result<PutOutcome, StoreError> {
        // Read and assemble before taking the lock, so neither a slow client nor the copy blocks
        // other requests.
        let chunks: Vec<Bytes> = exact_length(body, len)
            .try_collect()
            .await
            .map_err(StoreError::Body)?;
        let data = Bytes::from(chunks.concat());
        match self.entries.write().await.entry(key.clone()) {
            Entry::Occupied(_) => Ok(PutOutcome::AlreadyExists),
            Entry::Vacant(slot) => {
                slot.insert((data, writer.to_owned()));
                Ok(PutOutcome::Created)
            }
        }
    }

    async fn ready(&self) -> Result<(), StoreError> {
        Ok(())
    }
}
