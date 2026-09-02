use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio_stream::Stream;
use tonic::{Request, Response, Status, Streaming};

use crate::audit::AuditLog;
use crate::auth::AuthRegistry;
use crate::proto::gateway_server::Gateway;
use crate::proto::{
    AuthRequest, AuthResponse, SearchRequest, SearchResponse, TokenChunk, TokenStreamRequest,
    ToolMessage,
};
use crate::postgres::SearchIndex;
use crate::search::Hit;
use crate::spans;
use crate::Limiter;
use tracing::Instrument;

pub struct GatewayService {
    pub auth: AuthRegistry,
    pub store: SearchIndex,
    pub limiter: Arc<Limiter>,
    pub audit: AuditLog,
}

#[derive(Debug)]
pub enum SearchError {
    UnknownTenant,
    RateLimited,
    Internal(String),
}

impl GatewayService {
    pub fn authenticate_key(&self, tenant_key: &str) -> Option<(String, String, String)> {
        let span = tracing::info_span!(spans::AUTH);
        let _g = span.enter();
        let tenant = self.auth.authenticate(tenant_key)?;
        Some((tenant.id.clone(), tenant.slug.clone(), tenant.id.clone()))
    }

    pub fn resolve_tenant(&self, tenant: &str) -> bool {
        let span = tracing::info_span!(spans::TENANT_RESOLVE);
        let _g = span.enter();
        self.auth.known_tenant(tenant)
    }

    pub fn rate_limit_tenant(&self, tenant: &str) -> Result<(), SearchError> {
        let span = tracing::info_span!(spans::RATE_LIMIT);
        let _g = span.enter();
        match self.limiter.allow(tenant, 1) {
            Ok(true) => Ok(()),
            Ok(false) => Err(SearchError::RateLimited),
            Err(e) => Err(SearchError::Internal(e)),
        }
    }

    pub async fn search_docs(
        &self,
        tenant: &str,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<Hit>, SearchError> {
        if !self.resolve_tenant(tenant) {
            return Err(SearchError::UnknownTenant);
        }
        self.rate_limit_tenant(tenant)?;
        let hits = self.store.search(tenant, query, top_k).await;
        async {
            self.audit
                .emit(
                    tenant,
                    "search",
                    "",
                    &serde_json::json!({ "query": query, "hits": hits.len() }).to_string(),
                )
                .await;
        }
        .instrument(tracing::info_span!(spans::CLICKHOUSE_AUDIT))
        .await;
        Ok(hits)
    }

    pub async fn list_drive(&self, tenant: &str) -> Result<String, SearchError> {
        if !self.resolve_tenant(tenant) {
            return Err(SearchError::UnknownTenant);
        }
        self.rate_limit_tenant(tenant)?;
        let payload = async {
            let listed: Vec<_> = self
                .store
                .list(tenant)
                .await
                .into_iter()
                .map(|h| {
                    serde_json::json!({
                        "id": h.document_id,
                        "title": h.title,
                        "tenant_id": h.tenant_id,
                    })
                })
                .collect();
            serde_json::json!({ "files": listed, "tenant_id": tenant }).to_string()
        }
        .instrument(tracing::info_span!(spans::TOOL_ROUND_TRIP))
        .await;
        async {
            self.audit
                .emit(tenant, "tool", "drive.list", &payload)
                .await;
        }
        .instrument(tracing::info_span!(spans::CLICKHOUSE_AUDIT))
        .await;
        Ok(payload)
    }
}

fn tenant_id<T>(req: &Request<T>) -> Result<String, Status> {
    req.metadata()
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| Status::unauthenticated("missing x-tenant-id"))
}

fn search_status(err: SearchError) -> Status {
    match err {
        SearchError::UnknownTenant => Status::permission_denied("unknown tenant"),
        SearchError::RateLimited => Status::resource_exhausted("tenant rate limited"),
        SearchError::Internal(e) => Status::internal(e),
    }
}

fn hits_to_proto(hits: Vec<Hit>) -> SearchResponse {
    SearchResponse {
        hits: hits
            .into_iter()
            .map(|h| crate::proto::Citation {
                document_id: h.document_id,
                title: h.title,
                snippet: h.snippet,
                tenant_id: h.tenant_id,
            })
            .collect(),
    }
}

#[tonic::async_trait]
impl Gateway for GatewayService {
    async fn authenticate(
        &self,
        request: Request<AuthRequest>,
    ) -> Result<Response<AuthResponse>, Status> {
        let key = request.into_inner().tenant_key;
        let (id, slug, token) = self
            .authenticate_key(&key)
            .ok_or_else(|| Status::unauthenticated("unknown tenant key"))?;
        Ok(Response::new(AuthResponse {
            access_token: token,
            tenant_id: id,
            tenant_slug: slug,
        }))
    }

    async fn search(
        &self,
        request: Request<SearchRequest>,
    ) -> Result<Response<SearchResponse>, Status> {
        let tenant = tenant_id(&request)?;
        let body = request.into_inner();
        let top_k = if body.top_k == 0 { 5 } else { body.top_k as usize };
        let hits = self
            .search_docs(&tenant, &body.query, top_k)
            .await
            .map_err(search_status)?;
        Ok(Response::new(hits_to_proto(hits)))
    }

    type StreamTokensStream =
        Pin<Box<dyn Stream<Item = Result<TokenChunk, Status>> + Send + 'static>>;

    async fn stream_tokens(
        &self,
        request: Request<TokenStreamRequest>,
    ) -> Result<Response<Self::StreamTokensStream>, Status> {
        let tenant = tenant_id(&request)?;
        if !self.resolve_tenant(&tenant) {
            return Err(Status::permission_denied("unknown tenant"));
        }
        self.rate_limit_tenant(&tenant).map_err(search_status)?;
        let query = request.into_inner().query;
        let hits = self.store.search(&tenant, &query, 3).await;
        let mut answer = if hits.is_empty() {
            format!("No citations for this tenant on {query}.")
        } else {
            let titles: Vec<_> = hits.iter().map(|h| h.title.as_str()).collect();
            format!(
                "Answer for this tenant only. Query: {query}. Citations: {}.",
                titles.join("; ")
            )
        };
        for h in &hits {
            answer.push(' ');
            answer.push_str(&h.snippet);
        }
        let words: Vec<String> = answer.split_whitespace().map(|w| w.to_string()).collect();
        let stream = async_stream::stream! {
            let n = words.len();
            for (i, w) in words.into_iter().enumerate() {
                tokio::time::sleep(Duration::from_millis(12)).await;
                yield Ok(TokenChunk {
                    text: format!("{w} "),
                    done: i + 1 == n,
                });
            }
            if n == 0 {
                yield Ok(TokenChunk {
                    text: String::new(),
                    done: true,
                });
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }

    type ToolRoundTripStream =
        Pin<Box<dyn Stream<Item = Result<ToolMessage, Status>> + Send + 'static>>;

    async fn tool_round_trip(
        &self,
        request: Request<Streaming<ToolMessage>>,
    ) -> Result<Response<Self::ToolRoundTripStream>, Status> {
        let tenant = tenant_id(&request)?;
        if !self.resolve_tenant(&tenant) {
            return Err(Status::permission_denied("unknown tenant"));
        }
        self.rate_limit_tenant(&tenant).map_err(search_status)?;
        let store = self.store.clone();
        let audit = self.audit.clone();
        let mut inbound = request.into_inner();
        let stream = async_stream::try_stream! {
            while let Some(msg) = inbound.message().await? {
                let payload = async {
                    match msg.name.as_str() {
                    "drive.list" => {
                        let listed: Vec<_> = store
                            .list(&tenant)
                            .await
                            .into_iter()
                            .map(|h| {
                                serde_json::json!({
                                    "id": h.document_id,
                                    "title": h.title,
                                    "tenant_id": h.tenant_id,
                                })
                            })
                            .collect();
                        serde_json::json!({ "files": listed, "tenant_id": tenant }).to_string()
                    }
                    other => serde_json::json!({
                        "error": "unknown tool",
                        "name": other,
                        "tenant_id": tenant,
                    })
                    .to_string(),
                    }
                }
                .instrument(tracing::info_span!(spans::TOOL_ROUND_TRIP))
                .await;
                async {
                    audit
                        .emit(&tenant, "tool", &msg.name, &payload)
                        .await;
                }
                .instrument(tracing::info_span!(spans::CLICKHOUSE_AUDIT))
                .await;
                yield ToolMessage {
                    name: msg.name,
                    payload_json: payload,
                };
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }
}
