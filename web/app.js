const TENANTS = [
  { slug: "acme", key: "acme-demo-key", label: "Acme Corp" },
  { slug: "northwind", key: "northwind-demo-key", label: "Northwind" },
];

let active = TENANTS[0];
let token = "";

const tenantsEl = document.getElementById("tenants");
const hitsEl = document.getElementById("hits");
const errorEl = document.getElementById("error");
const statusEl = document.getElementById("status");
const streamEl = document.getElementById("stream");
const queryEl = document.getElementById("query");

function renderTenants() {
  tenantsEl.innerHTML = "";
  for (const t of TENANTS) {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = t.label;
    if (t.slug === active.slug) b.classList.add("active");
    b.addEventListener("click", () => {
      active = t;
      token = "";
      renderTenants();
    });
    tenantsEl.appendChild(b);
  }
}

async function authenticate() {
  const res = await fetch("/v1/auth", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ tenant_key: active.key }),
  });
  if (!res.ok) throw new Error("auth failed");
  const body = await res.json();
  token = body.access_token;
  statusEl.textContent = `${body.tenant_slug} · ${body.tenant_id.slice(0, 8)}`;
  return body;
}

async function search() {
  errorEl.textContent = "";
  hitsEl.innerHTML = "";
  streamEl.textContent = "Searching…";
  try {
    const session = await authenticate();
    const res = await fetch("/v1/search", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-tenant-id": session.tenant_id,
      },
      body: JSON.stringify({ query: queryEl.value, top_k: 8 }),
    });
    if (res.status === 429) throw new Error("rate limited");
    if (!res.ok) throw new Error(`search ${res.status}`);
    const hits = await res.json();
    if (!hits.length) {
      hitsEl.innerHTML = "<li><p>No citations for this tenant.</p></li>";
      streamEl.textContent = "No stream. Empty result set.";
      return;
    }
    for (const h of hits) {
      if (h.tenant_id !== session.tenant_id) {
        throw new Error("isolation failure: foreign tenant in hits");
      }
      const li = document.createElement("li");
      li.innerHTML = `<h3></h3><p></p>`;
      li.querySelector("h3").textContent = h.title;
      li.querySelector("p").textContent = h.snippet;
      hitsEl.appendChild(li);
    }
    streamEl.textContent = hits
      .map((h) => `${h.title}: ${h.snippet}`)
      .join("\n\n");
  } catch (err) {
    errorEl.textContent = String(err.message || err);
    streamEl.textContent = "";
  }
}

document.getElementById("run").addEventListener("click", search);
queryEl.addEventListener("keydown", (e) => {
  if (e.key === "Enter") search();
});
renderTenants();
