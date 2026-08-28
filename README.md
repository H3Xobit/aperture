# Aperture

Multi-tenant enterprise agent gateway. Aperture sits in front of connectors, collections, and tools. It authenticates a tenant, rate-limits the hot path in C++, searches only that tenant's documents, streams an answer, and records tool round-trips. The operator console is a desk for isolation, not a chatbot toy.

The screenshots below are the target product. The tree in this repository is the first working slice: proto, gRPC, REST, the C++ limiter, in-process tenant search, and a live console against two demo orgs.

## Operator desk

![Operator desk](docs/screenshots/desk.png)

Same query, two tenants. Citations stay inside the active org.

## Connectors and collections

![Connectors](docs/screenshots/connectors.png)

Drive, SharePoint, and a collection used as the retrieval surface. Connector OAuth is simulated. Tenant-scoped search is real in this slice (in-process). PostgreSQL RLS is the next wiring.

## Traces

![Traces](docs/screenshots/traces.png)

Target span tree for one agent request: auth, tenant resolve, C++ rate limit, RLS, collection search, tool round-trip, ClickHouse audit. OpenTelemetry lands in a later slice.

## What works today

1. `Authenticate` (gRPC unary and `POST /v1/auth`) maps a demo tenant key to a tenant id. This is not SSO.
2. `Search` is scoped by `x-tenant-id`. Acme never receives Northwind documents. Tests encode that invariant.
3. `StreamTokens` is a server stream. `ToolRoundTrip` is bidi; `drive.list` returns only that tenant's files.
4. The C++ token-bucket limiter is linked into the Rust gateway over a C ABI. Buckets are per tenant.
5. Postgres and ClickHouse schemas live in `sql/` and start from Compose. The gateway does not query them yet.
6. `python/embed` chunks text. Embeddings are not written anywhere yet.

## Simulated vs real

| Piece | Status |
| --- | --- |
| Tenant-scoped search | Real (in-process store, tested) |
| C++ token-bucket limiter | Real (C++ tests + Rust FFI tests) |
| gRPC unary, server stream, bidi | Real (tonic) |
| REST mapping for the desk | Real (axum) |
| Postgres RLS schema | Present, not wired to Search |
| ClickHouse `ORDER BY (tenant_id, …)` | Present, not wired to ingest |
| OAuth / SSO / SCIM | Simulated demo keys |
| Vault / CMEK | Out of scope |
| OpenTelemetry + Grafana | Documented target, not shipped |
| MCP production adapters | `drive.list` fixture only |

## Stack

- Rust (tonic gRPC gateway, axum HTTP)
- C++17 token-bucket limiter (`cpp/limiter`)
- PostgreSQL 16 (RLS, `FORCE ROW LEVEL SECURITY`)
- ClickHouse (MergeTree, `tenant_id` first in `ORDER BY`)
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
```

Optional databases:

```bash
docker compose up -d
```

## Ports

| Service | Host port |
| --- | --- |
| HTTP desk + API | 8080 |
| gRPC | 50051 |
| Postgres (compose) | 5432 |
| ClickHouse HTTP (compose) | 8123 |

## Isolation rule

A search for tenant A must not return a document, title, or snippet owned by tenant B. That check is in `crates/gateway/src/search.rs` and in the desk (`web/app.js`). Wiring the same rule through Postgres RLS is the next slice.

## Out of scope

Production SSO, customer connector tokens, CMEK, a hosted control plane, and claiming internet-scale throughput from this demo.

## License

MIT
