use aperture_gateway::audit::AuditLog;

const ACME: &str = "11111111-1111-1111-1111-111111111111";
const NORTHWIND: &str = "22222222-2222-2222-2222-222222222222";

async fn live_log() -> Option<AuditLog> {
    let url = std::env::var("APERTURE_CLICKHOUSE_URL").ok()?;
    match AuditLog::connect(&url).await {
        Ok(log) if log.using_clickhouse() => Some(log),
        Ok(_) => None,
        Err(e) => {
            eprintln!("skip clickhouse: {e}");
            None
        }
    }
}

#[tokio::test]
async fn acme_events_never_include_northwind() {
    let Some(log) = live_log().await else {
        return;
    };
    log.insert_event(ACME, "search", "", r#"{"query":"forecast"}"#)
        .await
        .expect("acme insert");
    log.insert_event(
        NORTHWIND,
        "tool",
        "drive.list",
        r#"{"tenant_id":"11111111-1111-1111-1111-111111111111"}"#,
    )
    .await
    .expect("northwind insert");

    let acme = log.list_for_tenant(ACME).await.expect("acme list");
    assert!(!acme.is_empty());
    assert!(acme.iter().all(|e| e.tenant_id == ACME));
    assert!(acme.iter().all(|e| e.tenant_id != NORTHWIND));

    let north = log.list_for_tenant(NORTHWIND).await.expect("northwind list");
    assert!(!north.is_empty());
    assert!(north.iter().all(|e| e.tenant_id == NORTHWIND));
}

#[tokio::test]
async fn insert_uses_auth_tenant_not_payload() {
    let Some(log) = live_log().await else {
        return;
    };
    log.insert_event(
        ACME,
        "tool",
        "drive.list",
        &format!(r#"{{"tenant_id":"{NORTHWIND}"}}"#),
    )
    .await
    .expect("insert");
    let acme = log.list_for_tenant(ACME).await.expect("list");
    assert!(acme.iter().any(|e| e.payload.contains(NORTHWIND)));
    assert!(acme.iter().all(|e| e.tenant_id == ACME));
}
