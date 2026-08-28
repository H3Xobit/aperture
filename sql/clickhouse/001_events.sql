CREATE DATABASE IF NOT EXISTS aperture;

CREATE TABLE IF NOT EXISTS aperture.tool_events
(
    tenant_id   UUID,
    event_time  DateTime64(3),
    event_id    UUID,
    event_type  LowCardinality(String),
    tool_name   LowCardinality(String),
    payload     String
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(event_time)
ORDER BY (tenant_id, event_time, event_id);

-- tenant_id is first in ORDER BY so per-tenant scans prune granules.
-- Row policy for the app role is applied in a later slice, once the gateway
-- sets a per-query tenant setting. Do not query this table as default if you
-- need isolation.
