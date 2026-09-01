use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Audit log for Search and ToolRoundTrip. Inserts use the authenticated
/// tenant id only. Payload `tenant_id` is stored as text and never trusted.
#[derive(Clone)]
pub struct AuditLog {
    client: Option<Client>,
}

#[derive(Debug, Clone, Row, Serialize, Deserialize)]
pub struct ToolEvent {
    #[serde(with = "clickhouse::serde::uuid")]
    pub tenant_id: Uuid,
    pub event_time: i64,
    #[serde(with = "clickhouse::serde::uuid")]
    pub event_id: Uuid,
    pub event_type: String,
    pub tool_name: String,
    pub payload: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventView {
    pub tenant_id: String,
    pub event_id: String,
    pub event_type: String,
    pub tool_name: String,
    pub payload: String,
}

impl AuditLog {
    pub fn disabled() -> Self {
        Self { client: None }
    }

    pub fn using_clickhouse(&self) -> bool {
        self.client.is_some()
    }

    pub async fn connect(url: &str) -> Result<Self, clickhouse::error::Error> {
        let client = Client::default()
            .with_url(url)
            .with_database("aperture");
        let _: u8 = client.query("SELECT 1").fetch_one().await?;
        Ok(Self {
            client: Some(client),
        })
    }

    /// Best-effort write. Failures are logged; callers still succeed.
    pub async fn emit(
        &self,
        tenant_id: &str,
        event_type: &str,
        tool_name: &str,
        payload: &str,
    ) {
        if self.client.is_none() {
            return;
        }
        match self.insert_event(tenant_id, event_type, tool_name, payload).await {
            Ok(()) => {}
            Err(e) => tracing::warn!(error = %e, "clickhouse ingest failed; rpc continues"),
        }
    }

    pub async fn insert_event(
        &self,
        tenant_id: &str,
        event_type: &str,
        tool_name: &str,
        payload: &str,
    ) -> Result<(), clickhouse::error::Error> {
        let client = self.client.as_ref().ok_or_else(|| {
            clickhouse::error::Error::Custom("clickhouse is not configured".into())
        })?;
        let row = build_event(tenant_id, event_type, tool_name, payload)?;
        let write = async {
            let mut insert = client.insert("tool_events")?;
            insert.write(&row).await?;
            insert.end().await?;
            Ok::<(), clickhouse::error::Error>(())
        };
        match tokio::time::timeout(Duration::from_secs(2), write).await {
            Ok(r) => r,
            Err(_) => Err(clickhouse::error::Error::Custom(
                "clickhouse insert timed out".into(),
            )),
        }
    }

    /// Reads only rows for `tenant_id`. The filter value is the authenticated
    /// tenant, not a body field. ORDER BY starts with tenant_id to match the
    /// table sort key.
    pub async fn list_for_tenant(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<EventView>, clickhouse::error::Error> {
        let client = self.client.as_ref().ok_or_else(|| {
            clickhouse::error::Error::Custom("clickhouse is not configured".into())
        })?;
        let tenant = Uuid::parse_str(tenant_id).map_err(|_| {
            clickhouse::error::Error::Custom("invalid tenant id".into())
        })?;
        let rows: Vec<ToolEvent> = client
            .query(
                "SELECT ?fields FROM tool_events
                 WHERE tenant_id = toUUID(?)
                 ORDER BY tenant_id, event_time, event_id
                 LIMIT 100",
            )
            .bind(tenant.to_string())
            .fetch_all()
            .await?;
        Ok(rows.into_iter().map(EventView::from).collect())
    }
}

impl From<ToolEvent> for EventView {
    fn from(e: ToolEvent) -> Self {
        Self {
            tenant_id: e.tenant_id.to_string(),
            event_id: e.event_id.to_string(),
            event_type: e.event_type,
            tool_name: e.tool_name,
            payload: e.payload,
        }
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn build_event(
    tenant_id: &str,
    event_type: &str,
    tool_name: &str,
    payload: &str,
) -> Result<ToolEvent, clickhouse::error::Error> {
    let tenant = Uuid::parse_str(tenant_id).map_err(|_| {
        clickhouse::error::Error::Custom("invalid tenant id".into())
    })?;
    Ok(ToolEvent {
        tenant_id: tenant,
        event_time: now_millis(),
        event_id: Uuid::new_v4(),
        event_type: event_type.to_string(),
        tool_name: tool_name.to_string(),
        payload: payload.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACME: &str = "11111111-1111-1111-1111-111111111111";
    const NORTHWIND: &str = "22222222-2222-2222-2222-222222222222";

    #[test]
    fn payload_tenant_id_is_not_used() {
        let payload = format!(r#"{{"tenant_id":"{NORTHWIND}","query":"forecast"}}"#);
        let ev = build_event(ACME, "search", "", &payload).unwrap();
        assert_eq!(ev.tenant_id.to_string(), ACME);
        assert!(ev.payload.contains(NORTHWIND));
    }

    #[test]
    fn disabled_log_does_not_require_clickhouse() {
        let log = AuditLog::disabled();
        assert!(!log.using_clickhouse());
    }
}
