use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Document {
    pub id: String,
    pub tenant_id: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub document_id: String,
    pub title: String,
    pub snippet: String,
    pub tenant_id: String,
}

#[derive(Clone, Debug)]
pub struct CollectionStore {
    docs: Vec<Document>,
}

impl CollectionStore {
    pub fn demo() -> Self {
        Self {
            docs: vec![
                Document {
                    id: "doc-acme-forecast".into(),
                    tenant_id: "11111111-1111-1111-1111-111111111111".into(),
                    title: "Acme Q3 forecast".into(),
                    body: "Industrial adhesives revenue up 12 percent. Plant A holds the confidential Acme SKU mix."
                        .into(),
                },
                Document {
                    id: "doc-acme-policy".into(),
                    tenant_id: "11111111-1111-1111-1111-111111111111".into(),
                    title: "Acme expense policy".into(),
                    body: "Acme employees submit receipts in the finance collection. Travel over 50,000 yen needs VP sign-off."
                        .into(),
                },
                Document {
                    id: "doc-nw-forecast".into(),
                    tenant_id: "22222222-2222-2222-2222-222222222222".into(),
                    title: "Northwind Q3 forecast".into(),
                    body: "Tea export volume declined 4 percent. Northwind warehouse B remains the only source for blend 7."
                        .into(),
                },
                Document {
                    id: "doc-nw-policy".into(),
                    tenant_id: "22222222-2222-2222-2222-222222222222".into(),
                    title: "Northwind expense policy".into(),
                    body: "Northwind field staff file expenses weekly. Blend formulas stay inside the Northwind collection."
                        .into(),
                },
            ],
        }
    }

    pub fn list(&self, tenant_id: &str) -> Vec<Hit> {
        if tenant_id.is_empty() {
            return Vec::new();
        }
        self.docs
            .iter()
            .filter(|d| d.tenant_id == tenant_id)
            .map(|d| Hit {
                document_id: d.id.clone(),
                title: d.title.clone(),
                snippet: d.body.clone(),
                tenant_id: d.tenant_id.clone(),
            })
            .collect()
    }

    pub fn search(&self, tenant_id: &str, query: &str, top_k: usize) -> Vec<Hit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() || tenant_id.is_empty() {
            return Vec::new();
        }
        let k = top_k.clamp(1, 20);
        self.docs
            .iter()
            .filter(|d| d.tenant_id == tenant_id)
            .filter(|d| {
                d.title.to_lowercase().contains(&q) || d.body.to_lowercase().contains(&q)
            })
            .take(k)
            .map(|d| Hit {
                document_id: d.id.clone(),
                title: d.title.clone(),
                snippet: d.body.clone(),
                tenant_id: d.tenant_id.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acme_search_never_returns_northwind() {
        let store = CollectionStore::demo();
        let hits = store.search(
            "11111111-1111-1111-1111-111111111111",
            "forecast",
            10,
        );
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.tenant_id.starts_with("1111")));
        assert!(hits.iter().all(|h| !h.snippet.to_lowercase().contains("northwind")));
        assert!(hits.iter().all(|h| !h.snippet.to_lowercase().contains("tea export")));
    }

    #[test]
    fn northwind_search_never_returns_acme() {
        let store = CollectionStore::demo();
        let hits = store.search(
            "22222222-2222-2222-2222-222222222222",
            "forecast",
            10,
        );
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.tenant_id.starts_with("2222")));
        assert!(hits.iter().all(|h| !h.snippet.to_lowercase().contains("acme")));
        assert!(hits.iter().all(|h| !h.snippet.to_lowercase().contains("adhesives")));
    }

    #[test]
    fn empty_tenant_returns_nothing() {
        let store = CollectionStore::demo();
        assert!(store.search("", "forecast", 10).is_empty());
        assert!(store.list("").is_empty());
    }

    #[test]
    fn list_is_tenant_scoped() {
        let store = CollectionStore::demo();
        let acme = store.list("11111111-1111-1111-1111-111111111111");
        assert_eq!(acme.len(), 2);
        assert!(acme.iter().all(|h| h.title.contains("Acme")));
        let north = store.list("22222222-2222-2222-2222-222222222222");
        assert_eq!(north.len(), 2);
        assert!(north.iter().all(|h| h.title.contains("Northwind")));
    }
}
