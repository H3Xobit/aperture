use aperture_gateway::postgres::SearchIndex;
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
