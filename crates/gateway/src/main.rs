use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use aperture_gateway::audit::AuditLog;
use aperture_gateway::auth::AuthRegistry;
use aperture_gateway::proto::gateway_server::GatewayServer;
use aperture_gateway::postgres::SearchIndex;
use aperture_gateway::search::CollectionStore;
use aperture_gateway::service::{GatewayService, SearchError};
use aperture_gateway::telemetry;
use aperture_gateway::Limiter;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

#[derive(Clone)]
struct AppState {
    auth: AuthRegistry,
    store: SearchIndex,
    limiter: Arc<Limiter>,
    audit: AuditLog,
}

#[derive(Deserialize)]
struct AuthBody {
    tenant_key: String,
}

#[derive(Serialize)]
struct AuthOut {
    access_token: String,
    tenant_id: String,
    tenant_slug: String,
}

#[derive(Deserialize)]
struct SearchBody {
    query: String,
    top_k: Option<u32>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: Option<String>,
}

fn tenant_from_headers(headers: &HeaderMap) -> Result<String, StatusCode> {
    if let Some(v) = headers.get("x-tenant-id").and_then(|v| v.to_str().ok()) {
        let t = v.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Some(v) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        let t = v.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

async fn healthz() -> &'static str {
    "ok\n"
}

async fn auth_handler(
    State(st): State<AppState>,
    Json(body): Json<AuthBody>,
) -> impl IntoResponse {
    let svc = gateway(&st);
    match svc.authenticate_key(&body.tenant_key) {
        Some((id, slug, token)) => Json(AuthOut {
            access_token: token,
            tenant_id: id,
            tenant_slug: slug,
        })
        .into_response(),
        None => (StatusCode::UNAUTHORIZED, "unknown tenant key").into_response(),
    }
}

fn gateway(st: &AppState) -> GatewayService {
    GatewayService {
        auth: st.auth.clone(),
        store: st.store.clone(),
        limiter: st.limiter.clone(),
        audit: st.audit.clone(),
    }
}

async fn search(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SearchBody>,
) -> impl IntoResponse {
    let tenant = match tenant_from_headers(&headers) {
        Ok(t) => t,
        Err(code) => return code.into_response(),
    };
    let svc = gateway(&st);
    let k = body.top_k.unwrap_or(5) as usize;
    match svc.search_docs(&tenant, &body.query, k).await {
        Ok(hits) => Json(hits).into_response(),
        Err(SearchError::UnknownTenant) => StatusCode::FORBIDDEN.into_response(),
        Err(SearchError::RateLimited) => StatusCode::TOO_MANY_REQUESTS.into_response(),
        Err(SearchError::Internal(_)) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn events(State(st): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let tenant = match tenant_from_headers(&headers) {
        Ok(t) => t,
        Err(code) => return code.into_response(),
    };
    if !st.auth.known_tenant(&tenant) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match st.audit.list_for_tenant(&tenant).await {
        Ok(rows) => {
            if rows.iter().any(|e| e.tenant_id != tenant) {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            Json(rows).into_response()
        }
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "clickhouse unavailable").into_response(),
    }
}

async fn search_get(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<SearchQuery>,
) -> impl IntoResponse {
    let body = SearchBody {
        query: q.q.unwrap_or_default(),
        top_k: Some(5),
    };
    search(State(st), headers, Json(body)).await
}

fn web_dir() -> PathBuf {
    std::env::var("APERTURE_WEB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("web"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    telemetry::install();

    let limiter = Limiter::new(40, 10.0)?;
    let auth_reg = AuthRegistry::demo();
    let memory = CollectionStore::demo();
    let store = match std::env::var("APERTURE_DATABASE_URL") {
        Ok(url) => match SearchIndex::connect(&url, memory.clone()).await {
            Ok(idx) => {
                tracing::info!("search reads postgres under RLS");
                idx
            }
            Err(e) => {
                tracing::warn!(error = %e, "postgres unavailable; search uses memory");
                SearchIndex::memory_only(memory)
            }
        },
        Err(_) => SearchIndex::memory_only(memory),
    };
    let audit = match std::env::var("APERTURE_CLICKHOUSE_URL") {
        Ok(url) => match AuditLog::connect(&url).await {
            Ok(log) => {
                tracing::info!("tool events write to clickhouse");
                log
            }
            Err(e) => {
                tracing::warn!(error = %e, "clickhouse unavailable; ingest skipped");
                AuditLog::disabled()
            }
        },
        Err(_) => AuditLog::disabled(),
    };
    let svc = GatewayService {
        auth: auth_reg.clone(),
        store: store.clone(),
        limiter: limiter.clone(),
        audit: audit.clone(),
    };
    let http_state = AppState {
        auth: auth_reg,
        store,
        limiter,
        audit,
    };

    let grpc_addr: SocketAddr = std::env::var("APERTURE_GRPC")
        .unwrap_or_else(|_| "0.0.0.0:50051".into())
        .parse()?;
    let http_addr: SocketAddr = std::env::var("APERTURE_HTTP")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;

    let grpc = tonic::transport::Server::builder()
        .add_service(GatewayServer::new(svc))
        .serve(grpc_addr);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let static_files = ServeDir::new(web_dir()).append_index_html_on_directories(true);
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/auth", post(auth_handler))
        .route("/v1/search", post(search).get(search_get))
        .route("/v1/events", get(events))
        .fallback_service(static_files)
        .layer(cors)
        .with_state(http_state);

    let listener = tokio::net::TcpListener::bind(http_addr).await?;
    tracing::info!(%grpc_addr, %http_addr, "aperture gateway");

    tokio::select! {
        r = grpc => r?,
        r = axum::serve(listener, app) => r?,
    }
    Ok(())
}
