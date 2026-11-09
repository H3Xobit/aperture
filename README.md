# Aperture

Multi-tenant enterprise agent gateway. Aperture sits in front of connectors, collections, and tools. It authenticates a tenant, rate-limits the hot path in C++, searches only that tenant's documents, streams an answer, and records tool round-trips. The operator console is a desk for isolation, not a chatbot toy.

The screenshots below are the target product. This slice exports OpenTelemetry spans for one agent request. Grafana is not included.

## Operator desk

![Operator desk](docs/screenshots/desk.png)

Same query, two tenants. Citations stay inside the active org.

## Connectors and collections

![Connectors](docs/screenshots/connectors.png)

Drive, SharePoint, and a collection used as the retrieval surface. Connector OAuth is simulated. Collection search is real: Postgres RLS when `APERTURE_DATABASE_URL` is set, otherwise the in-process store.

## Traces

![Traces](docs/screenshots/traces.png)

Target span tree for one agent request: Auth, TenantResolve, RateLimit, PostgresRLS or MemorySearch, CollectionSearch, ToolRoundTrip, ClickHouseAudit. Those names are live `tracing` spans. Set `APERTURE_OTLP_URL` to export OTLP HTTP to the Compose collector. Grafana is not shipped.

## What works today

1. `Authenticate` (gRPC unary and `POST /v1/auth`) maps a demo tenant key to a tenant id. This is not SSO.
2. `Search` is scoped by `x-tenant-id`. When Postgres is up the gateway role is `aperture_app` (no `BYPASSRLS`). Each request runs `SELECT set_config('app.tenant_id', $1, true)` inside a transaction. The SQL does not add `WHERE tenant_id`. If that tenant has `document_chunks` rows, snippets come from those rows. An empty or missing chunk table still searches `documents`. If Postgres is down, Search uses the in-process store. Acme never receives Northwind documents or chunks. Tests encode that invariant both in memory and against RLS.
3. `StreamTokens` is a server stream. `ToolRoundTrip` is bidi; `drive.list` returns only that tenant's files. Each tool call is written to ClickHouse when the audit URL is set.
4. The C++ token-bucket limiter is linked into the Rust gateway over a C ABI. Buckets are per tenant.
5. ClickHouse `tool_events` uses `ORDER BY (tenant_id, event_time, event_id)`. Inserts and `GET /v1/events` bind the authenticated tenant. If ClickHouse is down the RPC still succeeds.
6. Search, auth, rate limit, tool, and audit steps emit named spans. `APERTURE_OTLP_URL` sends them over OTLP HTTP. If the collector is down the RPC still succeeds.
7. `python/embed` chunks the four demo documents. When `APERTURE_DATABASE_URL` is set, the worker inserts `document_chunks` under the same transaction-local tenant GUC as Search. Embeddings are not computed yet.

## Simulated vs real

| Piece | Status |
| --- | --- |
| Tenant-scoped search | Real (chunk rows when present, else documents; memory fallback) |
| C++ token-bucket limiter | Real (C++ tests + Rust FFI tests) |
| gRPC unary, server stream, bidi | Real (tonic) |
| REST mapping for the desk | Real (axum) |
| Postgres RLS on Search | Real (`set_config` is_local, `aperture_app`, FORCE RLS) |
| ClickHouse tool events | Real (`ORDER BY tenant_id`, ingest + `GET /v1/events`) |
| OpenTelemetry traces | Real (named spans + OTLP HTTP). Grafana not shipped |
| OAuth / SSO / SCIM | Simulated demo keys |
| MCP production adapters | `drive.list` fixture only |

## Stack

- Rust (tonic gRPC gateway, axum HTTP)
- C++17 token-bucket limiter (`cpp/limiter`)
- PostgreSQL 16 (RLS, `FORCE ROW LEVEL SECURITY`)
- ClickHouse (MergeTree, `tenant_id` first in `ORDER BY`)
- OpenTelemetry (OTLP HTTP, named spans)
- Python 3.11 chunker
- TypeScript console source in `web/src` (the live desk is `web/`)

## Run

```bash
make test
make run
```

Desk: http://localhost:8080  
gRPC: localhost:50051  
Health: http://localhost:8080/healthz

Demo keys (public fixtures, not credentials):

| Tenant | Key |
| --- | --- |
| Acme Corp | `acme-demo-key` |
| Northwind | `northwind-demo-key` |

```bash
curl -s http://localhost:8080/v1/auth \
  -H 'content-type: application/json' \
  -d '{"tenant_key":"acme-demo-key"}'

curl -s http://localhost:8080/v1/search \
  -H 'content-type: application/json' \
  -H 'x-tenant-id: 11111111-1111-1111-1111-111111111111' \
  -d '{"query":"forecast","top_k":5}'

curl -s http://localhost:8080/v1/events \
  -H 'x-tenant-id: 11111111-1111-1111-1111-111111111111'
```

Optional Postgres (RLS path):

```bash
docker compose up -d postgres
export APERTURE_DATABASE_URL=postgres://aperture_app:aperture_app@127.0.0.1:5432/aperture
make run
```

Copy `.env.example` if you want the same values in a local file. Do not commit `.env`.

Traces (collector logs spans, no Grafana):

```bash
docker compose up -d otel-collector
export APERTURE_OTLP_URL=http://127.0.0.1:4318
make run
```

## Ports

| Service | Host port |
| --- | --- |
| HTTP desk + API | 8080 |
| gRPC | 50051 |
| Postgres (compose) | 5432 |
| ClickHouse HTTP (compose) | 8123 |
| OTLP HTTP (compose) | 4318 |

## Isolation rule

A search for tenant A must not return a document, title, or snippet owned by tenant B. Memory tests live in `crates/gateway/src/search.rs`. RLS tests live in `crates/gateway/tests/rls.rs` and run when `APERTURE_DATABASE_URL` is set (CI always sets it). ClickHouse event lists are tenant-scoped in `crates/gateway/tests/clickhouse.rs`. Span names for one Search-plus-tool request are asserted in `crates/gateway/tests/traces.rs`. The desk (`web/app.js`) rejects a foreign `tenant_id` in the response.

## Out of scope

Production SSO, customer connector tokens, CMEK, a hosted control plane, and claiming internet-scale throughput from this demo.

## License

MIT
