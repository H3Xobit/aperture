use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Tenant {
    pub id: String,
    pub slug: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct AuthRegistry {
    by_key: HashMap<String, Tenant>,
}

impl AuthRegistry {
    pub fn demo() -> Self {
        let acme = Tenant {
            id: "11111111-1111-1111-1111-111111111111".into(),
            slug: "acme".into(),
            name: "Acme Corp".into(),
        };
        let northwind = Tenant {
            id: "22222222-2222-2222-2222-222222222222".into(),
            slug: "northwind".into(),
            name: "Northwind".into(),
        };
        let mut by_key = HashMap::new();
        by_key.insert("acme-demo-key".into(), acme);
        by_key.insert("northwind-demo-key".into(), northwind);
        Self { by_key }
    }

    pub fn authenticate(&self, tenant_key: &str) -> Option<&Tenant> {
        self.by_key.get(tenant_key)
    }

    pub fn known_tenant(&self, tenant_id: &str) -> bool {
        self.by_key.values().any(|t| t.id == tenant_id)
    }
}
