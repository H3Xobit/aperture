use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::search::{CollectionStore, Hit};

/// In-process store plus optional Postgres. Search SQL does not filter
/// `tenant_id`; RLS does. Memory is used when the pool is missing or down.
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
        if self.pool.is_some() {
            match self.search_pg(tenant_id, query, top_k).await {
                Ok(hits) => return hits,
                Err(e) => tracing::warn!(error = %e, "postgres search failed; using memory"),
            }
        }
        self.memory.search(tenant_id, query, top_k)
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
