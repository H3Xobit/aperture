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
use crate::Limiter;

pub struct GatewayService {
    pub auth: AuthRegistry,
    pub store: SearchIndex,
    pub limiter: Arc<Limiter>,
    pub audit: AuditLog,
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

fn rate_limit(limiter: &Limiter, tenant: &str) -> Result<(), Status> {
    match limiter.allow(tenant, 1) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Status::resource_exhausted("tenant rate limited")),
        Err(e) => Err(Status::internal(e)),
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
        let tenant = self
            .auth
            .authenticate(&key)
            .ok_or_else(|| Status::unauthenticated("unknown tenant key"))?;
        Ok(Response::new(AuthResponse {
            access_token: tenant.id.clone(),
            tenant_id: tenant.id.clone(),
            tenant_slug: tenant.slug.clone(),
        }))
    }

    async fn search(
        &self,
        request: Request<SearchRequest>,
    ) -> Result<Response<SearchResponse>, Status> {
        let tenant = tenant_id(&request)?;
        if !self.auth.known_tenant(&tenant) {
            return Err(Status::permission_denied("unknown tenant"));
        }
        rate_limit(&self.limiter, &tenant)?;
        let body = request.into_inner();
        let top_k = if body.top_k == 0 { 5 } else { body.top_k as usize };
        let hits = self.store.search(&tenant, &body.query, top_k).await;
        self.audit
            .emit(
                &tenant,
                "search",
                "",
                &serde_json::json!({ "query": body.query, "hits": hits.len() }).to_string(),
            )
            .await;
        Ok(Response::new(hits_to_proto(hits)))
    }

    type StreamTokensStream =
        Pin<Box<dyn Stream<Item = Result<TokenChunk, Status>> + Send + 'static>>;

    async fn stream_tokens(
        &self,
        request: Request<TokenStreamRequest>,
    ) -> Result<Response<Self::StreamTokensStream>, Status> {
        let tenant = tenant_id(&request)?;
        if !self.auth.known_tenant(&tenant) {
            return Err(Status::permission_denied("unknown tenant"));
        }
        rate_limit(&self.limiter, &tenant)?;
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
        if !self.auth.known_tenant(&tenant) {
            return Err(Status::permission_denied("unknown tenant"));
        }
        rate_limit(&self.limiter, &tenant)?;
        let store = self.store.clone();
        let audit = self.audit.clone();
        let mut inbound = request.into_inner();
        let stream = async_stream::try_stream! {
            while let Some(msg) = inbound.message().await? {
                let payload = match msg.name.as_str() {
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
                };
                audit
                    .emit(&tenant, "tool", &msg.name, &payload)
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
