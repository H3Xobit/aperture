-- Aperture Postgres schema. Isolation is enforced with RLS, not application WHERE clauses.
-- The table owner is forced through the same policies (FORCE ROW LEVEL SECURITY).

CREATE TABLE tenants (
    id uuid PRIMARY KEY,
    slug text UNIQUE NOT NULL,
    name text NOT NULL
);

CREATE TABLE documents (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    title text NOT NULL,
    body text NOT NULL
);

INSERT INTO tenants (id, slug, name) VALUES
    ('11111111-1111-1111-1111-111111111111', 'acme', 'Acme Corp'),
    ('22222222-2222-2222-2222-222222222222', 'northwind', 'Northwind');

CREATE INDEX documents_tenant_id_idx ON documents (tenant_id);

INSERT INTO documents (id, tenant_id, title, body) VALUES
    (
        'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1',
        '11111111-1111-1111-1111-111111111111',
        'Acme Q3 forecast',
        'Industrial adhesives revenue up 12 percent. Plant A holds the confidential Acme SKU mix.'
    ),
    (
        'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa2',
        '11111111-1111-1111-1111-111111111111',
        'Acme expense policy',
        'Acme employees submit receipts in the finance collection. Travel over 50,000 yen needs VP sign-off.'
    ),
    (
        'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1',
        '22222222-2222-2222-2222-222222222222',
        'Northwind Q3 forecast',
        'Tea export volume declined 4 percent. Northwind warehouse B remains the only source for blend 7.'
    ),
    (
        'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb2',
        '22222222-2222-2222-2222-222222222222',
        'Northwind expense policy',
        'Northwind field staff file expenses weekly. Blend formulas stay inside the Northwind collection.'
    );

ALTER TABLE documents ENABLE ROW LEVEL SECURITY;
ALTER TABLE documents FORCE ROW LEVEL SECURITY;

CREATE POLICY documents_tenant ON documents
    USING (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid)
    WITH CHECK (tenant_id = NULLIF(current_setting('app.tenant_id', true), '')::uuid);

CREATE ROLE aperture_app LOGIN PASSWORD 'aperture_app' NOSUPERUSER NOBYPASSRLS;
GRANT CONNECT ON DATABASE aperture TO aperture_app;
GRANT USAGE ON SCHEMA public TO aperture_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON tenants, documents TO aperture_app;
