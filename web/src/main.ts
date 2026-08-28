type Tenant = {
  slug: string;
  key: string;
  label: string;
};

type Hit = {
  document_id: string;
  title: string;
  snippet: string;
  tenant_id: string;
};

const TENANTS: Tenant[] = [
  { slug: "acme", key: "acme-demo-key", label: "Acme Corp" },
  { slug: "northwind", key: "northwind-demo-key", label: "Northwind" },
];

export async function searchTenant(
  tenant: Tenant,
  query: string
): Promise<Hit[]> {
  const auth = await fetch("/v1/auth", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ tenant_key: tenant.key }),
  });
  if (!auth.ok) {
    throw new Error("auth failed");
  }
  const session = (await auth.json()) as {
    tenant_id: string;
    tenant_slug: string;
  };
  const res = await fetch("/v1/search", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-tenant-id": session.tenant_id,
    },
    body: JSON.stringify({ query, top_k: 8 }),
  });
  if (!res.ok) {
    throw new Error(`search ${res.status}`);
  }
  const hits = (await res.json()) as Hit[];
  for (const h of hits) {
    if (h.tenant_id !== session.tenant_id) {
      throw new Error("isolation failure: foreign tenant in hits");
    }
  }
  return hits;
}

export { TENANTS };
