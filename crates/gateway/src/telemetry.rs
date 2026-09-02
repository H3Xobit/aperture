use std::sync::OnceLock;
use std::time::Duration;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

static PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

/// Install fmt logs, and OTLP HTTP traces when `APERTURE_OTLP_URL` is set.
/// A down collector must not fail the process: export is batched with a
/// short timeout and errors stay in the exporter thread.
pub fn install() {
    let filter = EnvFilter::from_default_env().add_directive("info".parse().unwrap());
    match std::env::var("APERTURE_OTLP_URL") {
        Ok(url) if !url.trim().is_empty() => {
            if let Err(e) = install_otlp(filter.clone(), url.trim()) {
                tracing_subscriber::fmt().with_env_filter(filter).init();
                tracing::warn!(error = %e, "otlp exporter not installed; logs only");
            }
        }
        _ => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
    }
}

fn install_otlp(filter: EnvFilter, endpoint: &str) -> Result<(), Box<dyn std::error::Error>> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(2))
        .build()?;
    let resource = Resource::builder()
        .with_service_name("aperture-gateway")
        .build();
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();
    let tracer = provider.tracer("aperture-gateway");
    let otel = tracing_opentelemetry::layer().with_tracer(tracer);
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(otel)
        .init();
    let _ = PROVIDER.set(provider);
    tracing::info!(endpoint, "otlp http traces enabled");
    Ok(())
}
