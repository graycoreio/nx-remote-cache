//! Azure store tests against Azurite. Skipped unless `AZURITE_SAS_TOKEN` is set; see
//! `scripts/azurite-test.sh`, which starts Azurite, creates the container and runs these.
//!
//! Optional: `AZURITE_ENDPOINT` (default `http://127.0.0.1:10000/devstoreaccount1`) and
//! `AZURITE_CONTAINER` (default `nx-cache-test`).

use std::sync::Arc;

use bytes::Bytes;
use futures::{StreamExt, TryStreamExt, stream};
use nx_remote_cache::store::{
    ByteStream, CacheKey, CacheStore, PutOutcome, StoreError, WriteMeta,
    azure::{AzureBlobStore, AzureConfig},
};
use serde_json::json;

fn store_with(container: &str, block_size: usize) -> Option<AzureBlobStore> {
    std::env::var("AZURITE_SAS_TOKEN").ok()?;
    let endpoint = std::env::var("AZURITE_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:10000/devstoreaccount1".into());
    let config: AzureConfig = serde_json::from_value(json!({
        "endpoint": endpoint,
        "container": container,
        // Unique prefix per run so reruns against a long-lived Azurite don't collide.
        "prefix": format!("test-{}", uuid::Uuid::new_v4().simple()),
        "block_size": block_size,
        "auth": { "mode": "sas", "sas_token_env": "AZURITE_SAS_TOKEN" },
    }))
    .unwrap();
    Some(AzureBlobStore::new(&config).unwrap())
}

fn store(block_size: usize) -> Option<AzureBlobStore> {
    let container = std::env::var("AZURITE_CONTAINER").unwrap_or_else(|_| "nx-cache-test".into());
    store_with(&container, block_size)
}

macro_rules! require_azurite {
    ($e:expr) => {
        match $e {
            Some(store) => store,
            None => {
                eprintln!("skipping: AZURITE_SAS_TOKEN not set");
                return;
            }
        }
    };
}

/// Deterministic pseudo-random bytes, delivered in uneven chunks like a real request body.
fn payload(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
        .collect()
}

fn chunked(data: &[u8]) -> ByteStream {
    let chunks: Vec<Result<Bytes, std::io::Error>> = data
        .chunks(64 * 1024 + 7)
        .map(|c| Ok(Bytes::copy_from_slice(c)))
        .collect();
    stream::iter(chunks).boxed()
}

async fn read_all(store: &AzureBlobStore, key: &CacheKey) -> Option<(u64, Vec<u8>)> {
    let artifact = store.get(key).await.unwrap()?;
    let chunks: Vec<Bytes> = artifact.body.try_collect().await.unwrap();
    Some((artifact.len, chunks.concat()))
}

fn meta() -> WriteMeta {
    WriteMeta::from([("provider".into(), "test".into())])
}

#[tokio::test]
async fn ready_succeeds_with_valid_sas() {
    let store = require_azurite!(store(1024 * 1024));
    store.ready().await.unwrap();
}

#[tokio::test]
async fn ready_explains_unusable_container() {
    // A container-scoped SAS is refused on any other container (403); an account SAS reaches
    // the service and finds no such container (404). Either way the message must say why.
    let store = require_azurite!(store_with("no-such-container", 1024 * 1024));
    match store.ready().await {
        Err(StoreError::Backend(msg)) => {
            assert!(msg.contains("auth mode `sas`"), "{msg}");
            assert!(
                msg.contains("access denied") || msg.contains("container does not exist"),
                "{msg}"
            );
        }
        other => panic!("expected a backend error, got {other:?}"),
    }
}

#[tokio::test]
async fn small_roundtrip_and_conflict() {
    let store = require_azurite!(store(1024 * 1024));
    let key = CacheKey::parse("small").unwrap();
    assert!(store.get(&key).await.unwrap().is_none());

    let data = payload(1000);
    let outcome = store
        .put_if_absent(&key, data.len() as u64, chunked(&data), &meta())
        .await
        .unwrap();
    assert_eq!(outcome, PutOutcome::Created);
    assert_eq!(read_all(&store, &key).await.unwrap(), (1000, data));

    let other = payload(10);
    let outcome = store
        .put_if_absent(&key, 10, chunked(&other), &meta())
        .await
        .unwrap();
    assert_eq!(outcome, PutOutcome::AlreadyExists);
}

#[tokio::test]
async fn large_upload_is_staged_in_blocks_and_downloads_intact() {
    // 1 MiB blocks and a 10.5 MiB body: 11 staged blocks, and a download large enough for the
    // SDK to partition it (so the length must come from Content-Range, not Content-Length).
    let store = require_azurite!(store(1024 * 1024));
    let key = CacheKey::parse("large").unwrap();
    let data = payload(10 * 1024 * 1024 + 512 * 1024);
    let outcome = store
        .put_if_absent(&key, data.len() as u64, chunked(&data), &meta())
        .await
        .unwrap();
    assert_eq!(outcome, PutOutcome::Created);

    let (len, read) = read_all(&store, &key).await.unwrap();
    assert_eq!(len, data.len() as u64);
    assert!(read == data, "downloaded bytes differ");

    let again = store
        .put_if_absent(&key, data.len() as u64, chunked(&data), &meta())
        .await
        .unwrap();
    assert_eq!(again, PutOutcome::AlreadyExists);
}

#[tokio::test]
async fn concurrent_large_puts_have_exactly_one_winner() {
    let store = Arc::new(require_azurite!(store(256 * 1024)));
    let key = CacheKey::parse("race").unwrap();
    let tasks: Vec<_> = (0..6u8)
        .map(|i| {
            let (store, key) = (store.clone(), key.clone());
            tokio::spawn(async move {
                let mut data = payload(2 * 1024 * 1024);
                data[0] = i;
                store
                    .put_if_absent(&key, data.len() as u64, chunked(&data), &meta())
                    .await
                    .unwrap()
            })
        })
        .collect();
    let mut outcomes = Vec::new();
    for t in tasks {
        outcomes.push(t.await.unwrap());
    }
    assert_eq!(
        outcomes
            .iter()
            .filter(|o| **o == PutOutcome::Created)
            .count(),
        1,
        "{outcomes:?}"
    );

    // Whatever won, the stored blob is one writer's complete, unmixed body.
    let (len, read) = read_all(&store, &key).await.unwrap();
    assert_eq!(len, 2 * 1024 * 1024);
    let mut expected = payload(2 * 1024 * 1024);
    expected[0] = read[0];
    assert!(read == expected, "stored body mixes writers");
}

#[tokio::test]
async fn short_body_creates_nothing() {
    let store = require_azurite!(store(256 * 1024));
    for (name, len, declared) in [
        ("short-small", 100, 200u64),
        ("short-large", 600 * 1024, 700 * 1024),
    ] {
        let key = CacheKey::parse(name).unwrap();
        let data = payload(len);
        let result = store
            .put_if_absent(&key, declared, chunked(&data), &meta())
            .await;
        assert!(
            matches!(result, Err(StoreError::Body(_))),
            "{name}: {result:?}"
        );
        assert!(store.get(&key).await.unwrap().is_none(), "{name}");
    }
}

#[tokio::test]
async fn read_only_credential_is_reported_as_write_denied() {
    // A read-only identity (here: a read-only SAS) must come back as WriteDenied, which the
    // handler turns into a 403 that Nx skips, rather than a backend error that fails the run.
    if std::env::var("AZURITE_READONLY_SAS_TOKEN").is_err() {
        eprintln!("skipping: AZURITE_READONLY_SAS_TOKEN not set");
        return;
    }
    let endpoint = std::env::var("AZURITE_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:10000/devstoreaccount1".into());
    let config: AzureConfig = serde_json::from_value(json!({
        "endpoint": endpoint,
        "container": "nx-cache-test",
        "prefix": format!("test-{}", uuid::Uuid::new_v4().simple()),
        // 1 MiB blocks, so the 3 MiB case takes the staged-blocks path.
        "block_size": 1024 * 1024,
        "auth": { "mode": "sas", "sas_token_env": "AZURITE_READONLY_SAS_TOKEN" },
    }))
    .unwrap();
    let store = AzureBlobStore::new(&config).unwrap();
    store.ready().await.unwrap();

    let key = CacheKey::parse("readonly").unwrap();
    for len in [100, 3 * 1024 * 1024] {
        let data = payload(len);
        let result = store
            .put_if_absent(&key, len as u64, chunked(&data), &meta())
            .await;
        assert!(
            matches!(result, Err(StoreError::WriteDenied(_))),
            "{len} bytes: {result:?}"
        );
    }
    assert!(store.get(&key).await.unwrap().is_none());
}
