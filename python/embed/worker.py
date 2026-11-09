"""Chunk the demo documents and store them for the gateway."""

from __future__ import annotations

import os
import uuid
from collections import defaultdict

from chunker import chunk_text

ACME = "11111111-1111-1111-1111-111111111111"
NORTHWIND = "22222222-2222-2222-2222-222222222222"

# Same rows as sql/postgres/001_init.sql. Bodies stay in the source document.
DEMO_DOCUMENTS = (
    {
        "id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1",
        "tenant_id": ACME,
        "title": "Acme Q3 forecast",
        "body": (
            "Industrial adhesives revenue up 12 percent. "
            "Plant A holds the confidential Acme SKU mix."
        ),
    },
    {
        "id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa2",
        "tenant_id": ACME,
        "title": "Acme expense policy",
        "body": (
            "Acme employees submit receipts in the finance collection. "
            "Travel over 50,000 yen needs VP sign-off."
        ),
    },
    {
        "id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1",
        "tenant_id": NORTHWIND,
        "title": "Northwind Q3 forecast",
        "body": (
            "Tea export volume declined 4 percent. "
            "Northwind warehouse B remains the only source for blend 7."
        ),
    },
    {
        "id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb2",
        "tenant_id": NORTHWIND,
        "title": "Northwind expense policy",
        "body": (
            "Northwind field staff file expenses weekly. "
            "Blend formulas stay inside the Northwind collection."
        ),
    },
)


def plan_chunks(max_chars: int = 64, overlap: int = 16) -> list[dict]:
    rows: list[dict] = []
    for doc in DEMO_DOCUMENTS:
        parts = chunk_text(doc["body"], max_chars=max_chars, overlap=overlap)
        for index, part in enumerate(parts):
            rows.append(
                {
                    "id": str(uuid.uuid5(uuid.UUID(doc["id"]), f"chunk-{index}")),
                    "tenant_id": doc["tenant_id"],
                    "document_id": doc["id"],
                    "chunk_index": index,
                    "title": doc["title"],
                    "body": part,
                }
            )
    return rows


def write_chunks(url: str, rows: list[dict]) -> None:
    """Replace each tenant's chunks inside a transaction-local GUC.

    set_config(..., true) is LOCAL, so the tenant id does not leak to the
    next transaction on a pooled connection. WITH CHECK rejects a row whose
    tenant_id does not match that setting.
    """
    import psycopg

    grouped: dict[str, list[dict]] = defaultdict(list)
    for row in rows:
        grouped[row["tenant_id"]].append(row)
    with psycopg.connect(url, autocommit=True) as conn:
        for tenant_id, group in grouped.items():
            with conn.transaction():
                conn.execute(
                    "SELECT set_config('app.tenant_id', %s, true)",
                    (tenant_id,),
                )
                conn.execute("DELETE FROM document_chunks")
                for row in group:
                    conn.execute(
                        """
                        INSERT INTO document_chunks
                            (id, tenant_id, document_id, chunk_index, title, body)
                        VALUES (%s::uuid, %s::uuid, %s::uuid, %s, %s, %s)
                        """,
                        (
                            row["id"],
                            row["tenant_id"],
                            row["document_id"],
                            row["chunk_index"],
                            row["title"],
                            row["body"],
                        ),
                    )


def main() -> None:
    rows = plan_chunks()
    url = os.environ.get("APERTURE_DATABASE_URL", "").strip()
    if not url:
        for row in rows:
            print(f"{row['chunk_index']}\t{row['tenant_id']}\t{row['body']}")
        return
    write_chunks(url, rows)


if __name__ == "__main__":
    main()
