use std::sync::{Arc, Mutex};

use aperture_gateway::audit::AuditLog;
use aperture_gateway::auth::AuthRegistry;
use aperture_gateway::postgres::SearchIndex;
use aperture_gateway::search::CollectionStore;
use aperture_gateway::service::GatewayService;
use aperture_gateway::spans;
use aperture_gateway::Limiter;
use tracing::Subscriber;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};

struct NameLayer(Arc<Mutex<Vec<String>>>);

impl<S> Layer<S> for NameLayer
where
    S: Subscriber,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: Context<'_, S>,
    ) {
        self.0
            .lock()
            .expect("span names")
            .push(attrs.metadata().name().to_string());
    }
}

fn demo_service() -> GatewayService {
    GatewayService {
        auth: AuthRegistry::demo(),
        store: SearchIndex::memory_only(CollectionStore::demo()),
        limiter: Limiter::new(40, 10.0).expect("limiter"),
        audit: AuditLog::disabled(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn search_request_emits_span_names() {
    let names = Arc::new(Mutex::new(Vec::new()));
    let subscriber = Registry::default().with(NameLayer(names.clone()));
    let _guard = tracing::subscriber::set_default(subscriber);
    let svc = demo_service();
    let acme = "11111111-1111-1111-1111-111111111111";
    assert!(svc.authenticate_key("acme-demo-key").is_some());
    let hits = svc
        .search_docs(acme, "forecast", 5)
        .await
        .expect("search");
    assert!(!hits.is_empty());
    let listed = svc.list_drive(acme).await.expect("drive.list");
    assert!(listed.contains("Acme"));
    assert!(!listed.to_lowercase().contains("northwind"));
    drop(_guard);

    let got = names.lock().expect("names").clone();
    for required in [
        spans::AUTH,
        spans::TENANT_RESOLVE,
        spans::RATE_LIMIT,
        spans::MEMORY_SEARCH,
        spans::COLLECTION_SEARCH,
        spans::CLICKHOUSE_AUDIT,
        spans::TOOL_ROUND_TRIP,
    ] {
        assert!(
            got.iter().any(|n| n == required),
            "missing span {required} in {got:?}"
        );
    }
}
