//! In-process store for tests and local smoke runs. Not persistent.

use std::collections::{HashMap, hash_map::Entry};

use async_trait::async_trait;
use bytes::Bytes;
use futures::{StreamExt, TryStreamExt, stream};
use tokio::sync::RwLock;

use super::{
    Artifact, ByteStream, CacheKey, CacheStore, PutOutcome, StoreError, WriteMeta, exact_length,
};

#[derive(Default)]
pub struct MemoryStore {
    entries: RwLock<HashMap<CacheKey, (Bytes, WriteMeta)>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn meta(&self, key: &CacheKey) -> Option<WriteMeta> {
        self.entries.read().await.get(key).map(|(_, m)| m.clone())
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
        body: ByteStream,
        meta: &WriteMeta,
    ) -> Result<PutOutcome, StoreError> {
        // Buffer before taking the lock so a slow client can't block other requests.
        let chunks: Vec<Bytes> = exact_length(body, len)
            .try_collect()
            .await
            .map_err(StoreError::Body)?;
        match self.entries.write().await.entry(key.clone()) {
            Entry::Occupied(_) => Ok(PutOutcome::AlreadyExists),
            Entry::Vacant(slot) => {
                slot.insert((Bytes::from(chunks.concat()), meta.clone()));
                Ok(PutOutcome::Created)
            }
        }
    }

    async fn ready(&self) -> Result<(), StoreError> {
        Ok(())
    }
}
