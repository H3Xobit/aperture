import os
import unittest

from worker import ACME, NORTHWIND, DEMO_DOCUMENTS, plan_chunks, write_chunks


class PlanChunkTests(unittest.TestCase):
    def test_acme_text_never_labeled_northwind(self) -> None:
        rows = plan_chunks()
        acme_bits = [
            row
            for row in rows
            if "adhesives" in row["body"].lower() or "sku mix" in row["body"].lower()
        ]
        self.assertTrue(acme_bits)
        self.assertTrue(all(row["tenant_id"] == ACME for row in acme_bits))
        north = [row for row in rows if row["tenant_id"] == NORTHWIND]
        self.assertTrue(north)
        for row in north:
            body = row["body"].lower()
            self.assertNotIn("adhesives", body)
            self.assertNotIn("acme", body)

    def test_each_chunk_is_a_slice_of_its_document(self) -> None:
        bodies = {doc["id"]: " ".join(doc["body"].split()) for doc in DEMO_DOCUMENTS}
        rows = plan_chunks(max_chars=64, overlap=16)
        self.assertGreater(len(rows), len(DEMO_DOCUMENTS))
        seen_split = False
        previous: dict | None = None
        for row in rows:
            self.assertIn(row["body"], bodies[row["document_id"]])
            if (
                previous is not None
                and previous["document_id"] == row["document_id"]
                and row["chunk_index"] == previous["chunk_index"] + 1
            ):
                seen_split = True
                self.assertTrue(row["body"].startswith(previous["body"][-16:]))
            previous = row
        self.assertTrue(seen_split)

    def test_write_roundtrip_hides_acme_from_northwind(self) -> None:
        url = os.environ.get("APERTURE_DATABASE_URL", "").strip()
        if not url:
            self.skipTest("APERTURE_DATABASE_URL not set")
        try:
            import psycopg
        except ImportError:
            self.skipTest("psycopg is not installed")
        rows = plan_chunks()
        write_chunks(url, rows)
        try:
            with psycopg.connect(url, autocommit=True) as conn:
                with conn.transaction():
                    conn.execute(
                        "SELECT set_config('app.tenant_id', %s, true)",
                        (NORTHWIND,),
                    )
                    leaked = conn.execute(
                        "SELECT body FROM document_chunks WHERE body ILIKE %s",
                        ("%adhesives%",),
                    ).fetchall()
                self.assertEqual(leaked, [])
                with conn.transaction():
                    conn.execute(
                        "SELECT set_config('app.tenant_id', %s, true)",
                        (ACME,),
                    )
                    found = conn.execute(
                        "SELECT count(*) FROM document_chunks WHERE body ILIKE %s",
                        ("%adhesives%",),
                    ).fetchone()
                self.assertIsNotNone(found)
                assert found is not None
                self.assertGreater(found[0], 0)
        finally:
            with psycopg.connect(url, autocommit=True) as conn:
                for tenant in (ACME, NORTHWIND):
                    with conn.transaction():
                        conn.execute(
                            "SELECT set_config('app.tenant_id', %s, true)",
                            (tenant,),
                        )
                        conn.execute("DELETE FROM document_chunks")


if __name__ == "__main__":
    unittest.main()
