use aperture_gateway::postgres::{ChunkWrite, SearchIndex};
use aperture_gateway::search::CollectionStore;

const ACME: &str = "11111111-1111-1111-1111-111111111111";
const NORTHWIND: &str = "22222222-2222-2222-2222-222222222222";

async fn live_index() -> Option<SearchIndex> {
    let url = std::env::var("APERTURE_DATABASE_URL").ok()?;
    match SearchIndex::connect(&url, CollectionStore::demo()).await {
        Ok(idx) if idx.using_postgres() => Some(idx),
        Ok(_) => None,
        Err(e) => {
            eprintln!("skip rls: {e}");
            None
        }
    }
}

#[tokio::test]
async fn northwind_session_cannot_read_acme_adhesives() {
    let Some(idx) = live_index().await else {
        return;
    };
    let hits = idx
        .search_pg(NORTHWIND, "adhesives", 10)
        .await
        .expect("postgres search");
    assert!(
        hits.is_empty(),
        "Northwind GUC must not surface Acme adhesives: {hits:?}"
    );
}

#[tokio::test]
async fn acme_session_cannot_read_northwind_tea() {
    let Some(idx) = live_index().await else {
        return;
    };
    let hits = idx
        .search_pg(ACME, "tea export", 10)
        .await
        .expect("postgres search");
    assert!(
        hits.is_empty(),
        "Acme GUC must not surface Northwind tea: {hits:?}"
    );
}

#[tokio::test]
async fn acme_session_reads_only_acme_forecast() {
    let Some(idx) = live_index().await else {
        return;
    };
    let hits = idx
        .search_pg(ACME, "forecast", 10)
        .await
        .expect("postgres search");
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.tenant_id == ACME));
    assert!(hits.iter().all(|h| !h.snippet.to_lowercase().contains("northwind")));
}

#[tokio::test]
async fn unset_tenant_guc_returns_no_rows() {
    let Some(idx) = live_index().await else {
        return;
    };
    let hits = idx
        .search_pg("", "forecast", 10)
        .await
        .expect("empty tenant short-circuits");
    assert!(hits.is_empty());
}

const ACME_DOC: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1";
const NORTH_DOC: &str = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1";
const ACME_TOKEN: &str = "acme-only-chunk-token";
const NORTH_TOKEN: &str = "northwind-only-chunk-token";

#[tokio::test]
async fn northwind_read_never_returns_acme_chunks() {
    let Some(idx) = live_index().await else {
        return;
    };
    let _ = idx.replace_chunks(ACME, &[]).await;
    let _ = idx.replace_chunks(NORTHWIND, &[]).await;

    let docs = idx.search(ACME, "adhesives", 10).await;
    assert!(
        !docs.is_empty(),
        "search must still return documents when no chunks are stored"
    );
    assert!(docs.iter().all(|h| h.tenant_id == ACME));
    assert!(docs
        .iter()
        .any(|h| h.snippet.to_lowercase().contains("adhesives")));

    idx.replace_chunks(
        ACME,
        &[ChunkWrite {
            document_id: ACME_DOC.into(),
            chunk_index: 0,
            title: "Acme Q3 forecast".into(),
            body: format!("Plant note {ACME_TOKEN}"),
        }],
    )
    .await
    .expect("seed acme chunks");
    idx.replace_chunks(
        NORTHWIND,
        &[ChunkWrite {
            document_id: NORTH_DOC.into(),
            chunk_index: 0,
            title: "Northwind Q3 forecast".into(),
            body: format!("Warehouse note {NORTH_TOKEN}"),
        }],
    )
    .await
    .expect("seed northwind chunks");

    let north_own = idx.search(NORTHWIND, NORTH_TOKEN, 10).await;
    assert!(
        north_own.len() == 1 && north_own[0].snippet.contains(NORTH_TOKEN),
        "northwind should read its own chunk, got {north_own:?}"
    );
    let leaked = idx.search(NORTHWIND, ACME_TOKEN, 10).await;
    assert!(
        leaked.is_empty(),
        "Northwind read returned Acme chunks: {leaked:?}"
    );
    let acme = idx.search(ACME, ACME_TOKEN, 10).await;
    assert_eq!(acme.len(), 1, "{acme:?}");
    assert_eq!(acme[0].tenant_id, ACME);
    assert!(acme[0].snippet.contains(ACME_TOKEN));
    assert!(!acme[0].snippet.to_lowercase().contains("northwind"));

    idx.replace_chunks(ACME, &[])
        .await
        .expect("clear acme chunks");
    idx.replace_chunks(NORTHWIND, &[])
        .await
        .expect("clear northwind chunks");
}
