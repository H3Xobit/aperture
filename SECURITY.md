# Security

Report vulnerabilities privately to the repository owner. Do not open a public issue for a live exploit.

## Demo keys

The tokens in this repository (`acme-demo-key`, `northwind-demo-key`) are public fixtures. They are not production credentials.

## What isolation actually enforces

When `APERTURE_DATABASE_URL` points at `aperture_app`, Search opens a transaction, calls `set_config('app.tenant_id', $1, true)`, and reads `documents` with no application `WHERE tenant_id`. RLS plus `FORCE ROW LEVEL SECURITY` is the isolation boundary. The compose superuser bypasses RLS; do not point the gateway at that role.

If Postgres is unset or down, Search falls back to the in-process store.

When `APERTURE_CLICKHOUSE_URL` is set, Search and ToolRoundTrip insert into `aperture.tool_events`. The tenant column is the authenticated tenant, never a payload field. `GET /v1/events` lists only that tenant. If ClickHouse is down, the RPC still succeeds.

When `APERTURE_OTLP_URL` is set, spans export over OTLP HTTP. Export is batched with a two-second timeout. If the collector is down, the RPC still succeeds.

## Secrets

Do not commit `.env`, cloud keys, or customer connector tokens. Compose defaults are local-only passwords.
