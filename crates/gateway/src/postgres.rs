use std::collections::BTreeSet;

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use tracing::Instrument;

use crate::search::{CollectionStore, Hit};
use crate::spans;

#[derive(Clone, Debug)]
pub struct ChunkWrite {
    pub document_id: String,
    pub chunk_index: i32,
    pub title: String,
    pub body: String,
}

/// In-process store plus optional Postgres. Search SQL does not filter
/// `tenant_id`; RLS does. When the tenant has chunk rows, Search reads
/// those. Otherwise it reads documents. Memory is used when the pool is
/// missing or down.
#[derive(Clone)]
pub struct SearchIndex {
    memory: CollectionStore,
    pool: Option<PgPool>,
}

impl SearchIndex {
    pub fn memory_only(memory: CollectionStore) -> Self {
        Self { memory, pool: None }
    }

    pub fn using_postgres(&self) -> bool {
        self.pool.is_some()
    }

    pub async fn connect(url: &str, memory: CollectionStore) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(url)
            .await?;
        let bypasses: bool = sqlx::query_scalar(
            "SELECT COALESCE(rolsuper OR rolbypassrls, true)
             FROM pg_roles
             WHERE rolname = current_user",
        )
        .fetch_one(&pool)
        .await?;
        if bypasses {
            return Err(sqlx::Error::Configuration(
                "database role bypasses RLS; use aperture_app".into(),
            ));
        }
        Ok(Self {
            memory,
            pool: Some(pool),
        })
    }

    pub async fn search(&self, tenant_id: &str, query: &str, top_k: usize) -> Vec<Hit> {
        async {
            if self.pool.is_some() {
                match self
                    .search_pg_prefer_chunks(tenant_id, query, top_k)
                    .instrument(tracing::info_span!(spans::POSTGRES_RLS))
                    .await
                {
                    Ok(hits) => return hits,
                    Err(e) => tracing::warn!(error = %e, "postgres search failed; using memory"),
                }
            }
            async { self.memory.search(tenant_id, query, top_k) }
                .instrument(tracing::info_span!(spans::MEMORY_SEARCH))
                .await
        }
        .instrument(tracing::info_span!(spans::COLLECTION_SEARCH))
        .await
    }

    pub async fn list(&self, tenant_id: &str) -> Vec<Hit> {
        if self.pool.is_some() {
            match self.list_pg(tenant_id).await {
                Ok(hits) => return hits,
                Err(e) => tracing::warn!(error = %e, "postgres list failed; using memory"),
            }
        }
        self.memory.list(tenant_id)
    }

    /// Postgres path only. Used by isolation tests so a memory fallback
    /// cannot hide a broken policy.
    pub async fn search_pg(
        &self,
        tenant_id: &str,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<Hit>, sqlx::Error> {
        let pool = self.pool.as_ref().ok_or_else(|| {
            sqlx::Error::Configuration("postgres pool is not configured".into())
        })?;
        let q = query.trim();
        if q.is_empty() || tenant_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config($1, $2, true)")
            .bind("app.tenant_id")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
        let pattern = like_contains(q);
        let limit = i64::try_from(top_k.clamp(1, 20)).unwrap_or(5);
        let rows = sqlx::query(
            r#"SELECT id::text, tenant_id::text, title, body
               FROM documents
               WHERE title ILIKE $1 ESCAPE '\'
                  OR body ILIKE $1 ESCAPE '\'
               LIMIT $2"#,
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows_to_hits(rows))
    }

    /// Chunk rows when this tenant has any. Rank those rows by how many
    /// distinct query tokens appear in the title or body, then by
    /// chunk_index. A row that shares none is left out. Documents keep
    /// substring search when the chunk table is missing or this tenant has
    /// no chunks. SQL does not filter `tenant_id`.
    async fn search_pg_prefer_chunks(
        &self,
        tenant_id: &str,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<Hit>, sqlx::Error> {
        let pool = self.pool.as_ref().ok_or_else(|| {
            sqlx::Error::Configuration("postgres pool is not configured".into())
        })?;
        let q = query.trim();
        if q.is_empty() || tenant_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config($1, $2, true)")
            .bind("app.tenant_id")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
        let chunks_ready: bool = sqlx::query_scalar(
            "SELECT to_regclass('public.document_chunks') IS NOT NULL",
        )
        .fetch_one(&mut *tx)
        .await?;
        if chunks_ready {
            let n: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM document_chunks")
                .fetch_one(&mut *tx)
                .await?;
            if n > 0 {
                let rows = sqlx::query(
                    r#"SELECT document_id::text, tenant_id::text, title, body, chunk_index
                       FROM document_chunks
                       ORDER BY chunk_index, document_id"#,
                )
                .fetch_all(&mut *tx)
                .await?;
                tx.commit().await?;
                return Ok(rank_chunks_by_overlap(rows, q, top_k));
            }
        }
        let pattern = like_contains(q);
        let limit = i64::try_from(top_k.clamp(1, 20)).unwrap_or(5);
        let rows = sqlx::query(
            r#"SELECT id::text, tenant_id::text, title, body
               FROM documents
               WHERE title ILIKE $1 ESCAPE '\'
                  OR body ILIKE $1 ESCAPE '\'
               LIMIT $2"#,
        )
        .bind(&pattern)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows_to_hits(rows))
    }

    /// Replace the chunks visible to `tenant_id`. An empty slice deletes them.
    pub async fn replace_chunks(
        &self,
        tenant_id: &str,
        chunks: &[ChunkWrite],
    ) -> Result<(), sqlx::Error> {
        let pool = self.pool.as_ref().ok_or_else(|| {
            sqlx::Error::Configuration("postgres pool is not configured".into())
        })?;
        if tenant_id.is_empty() {
            return Err(sqlx::Error::Configuration("tenant_id is empty".into()));
        }
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config($1, $2, true)")
            .bind("app.tenant_id")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM document_chunks")
            .execute(&mut *tx)
            .await?;
        for chunk in chunks {
            sqlx::query(
                r#"INSERT INTO document_chunks
                       (id, tenant_id, document_id, chunk_index, title, body)
                   VALUES (gen_random_uuid(), $1::uuid, $2::uuid, $3, $4, $5)"#,
            )
            .bind(tenant_id)
            .bind(&chunk.document_id)
            .bind(chunk.chunk_index)
            .bind(&chunk.title)
            .bind(&chunk.body)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn list_pg(&self, tenant_id: &str) -> Result<Vec<Hit>, sqlx::Error> {
        let pool = self.pool.as_ref().ok_or_else(|| {
            sqlx::Error::Configuration("postgres pool is not configured".into())
        })?;
        if tenant_id.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config($1, $2, true)")
            .bind("app.tenant_id")
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
        let rows = sqlx::query(
            "SELECT id::text, tenant_id::text, title, body FROM documents",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows_to_hits(rows))
    }
}

fn rows_to_hits(rows: Vec<sqlx::postgres::PgRow>) -> Vec<Hit> {
    rows.into_iter()
        .map(|r| Hit {
            document_id: r.get(0),
            title: r.get(2),
            snippet: r.get(3),
            tenant_id: r.get(1),
        })
        .collect()
}

fn terms(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            for lower in ch.to_lowercase() {
                cur.push(lower);
            }
        } else if !cur.is_empty() {
            out.insert(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.insert(cur);
    }
    out
}

fn rank_chunks_by_overlap(rows: Vec<sqlx::postgres::PgRow>, query: &str, top_k: usize) -> Vec<Hit> {
    let chunks = rows
        .into_iter()
        .map(|row| {
            let chunk_index: i32 = row.get(4);
            let hit = Hit {
                document_id: row.get(0),
                tenant_id: row.get(1),
                title: row.get(2),
                snippet: row.get(3),
            };
            (chunk_index, hit)
        })
        .collect();
    rank_chunk_hits(chunks, query, top_k)
}

fn rank_chunk_hits(chunks: Vec<(i32, Hit)>, query: &str, top_k: usize) -> Vec<Hit> {
    let query_terms = terms(query);
    let mut scored: Vec<(usize, i32, Hit)> = Vec::new();
    for (chunk_index, hit) in chunks {
        let passage = format!("{} {}", hit.title, hit.snippet);
        let score = query_terms.intersection(&terms(&passage)).count();
        if score == 0 {
            continue;
        }
        scored.push((score, chunk_index, hit));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let k = top_k.clamp(1, 20);
    scored.into_iter().take(k).map(|(_, _, hit)| hit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acme_chunk(index: i32, body: &str) -> (i32, Hit) {
        (
            index,
            Hit {
                document_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1".into(),
                title: "Acme Q3 forecast".into(),
                snippet: body.into(),
                tenant_id: "11111111-1111-1111-1111-111111111111".into(),
            },
        )
    }

    #[test]
    fn more_shared_terms_rank_ahead_of_an_earlier_chunk() {
        let hits = rank_chunk_hits(
            vec![
                acme_chunk(0, "Plant A mentions adhesives in passing."),
                acme_chunk(
                    1,
                    "The confidential plan lists the SKU mix and adhesives together.",
                ),
                acme_chunk(2, "Tea export volume declined."),
            ],
            "adhesives sku mix",
            10,
        );
        assert_eq!(hits.len(), 2);
        assert!(hits[0].snippet.contains("SKU mix"));
        assert!(hits[1].snippet.contains("in passing"));
        assert!(hits
            .iter()
            .all(|h| h.document_id == "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"));
        assert!(hits
            .iter()
            .all(|h| !h.snippet.to_lowercase().contains("tea")));
    }

    #[test]
    fn repeated_query_words_tie_on_chunk_index() {
        let hits = rank_chunk_hits(
            vec![
                acme_chunk(2, "adhesives adhesives adhesives"),
                acme_chunk(0, "Earlier adhesives note."),
            ],
            "adhesives adhesives",
            10,
        );
        assert_eq!(hits.len(), 2);
        assert!(hits[0].snippet.contains("Earlier"));
        assert!(hits[1].snippet.contains("adhesives adhesives adhesives"));
    }
}

fn like_contains(q: &str) -> String {
    let mut out = String::from("%");
    for c in q.chars() {
        match c {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('%');
    out
}
