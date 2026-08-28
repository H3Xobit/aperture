# Security

Report vulnerabilities privately to the repository owner. Do not open a public issue for a live exploit.

## Demo keys

The tokens in this repository (`acme-demo-key`, `northwind-demo-key`) are public fixtures. They are not production credentials.

## What isolation actually enforces

Collection search is tenant-scoped in the gateway process. PostgreSQL row-level security and ClickHouse row policies are in `sql/` and are the next slice. Until those are wired, do not treat this tree as a multi-tenant production control plane.

## Secrets

Do not commit `.env`, cloud keys, or customer connector tokens. Compose defaults are local-only passwords.
