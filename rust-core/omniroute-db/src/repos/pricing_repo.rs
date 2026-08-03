use anyhow::Result;
use rusqlite::{Connection, params};

/// Pricing entry: cost per million tokens for a provider+model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PricingRow {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

pub fn get_all(conn: &Connection) -> Result<Vec<PricingRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, provider, model, input_per_mtok, output_per_mtok FROM pricing ORDER BY provider, model",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PricingRow {
            id: row.get(0)?,
            provider: row.get(1)?,
            model: row.get(2)?,
            input_per_mtok: row.get(3)?,
            output_per_mtok: row.get(4)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn upsert(conn: &Connection, p: &PricingRow) -> Result<()> {
    conn.execute(
        "INSERT INTO pricing (id, provider, model, input_per_mtok, output_per_mtok)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(provider, model) DO UPDATE SET
           input_per_mtok=?4, output_per_mtok=?5, updated_at=datetime('now')",
        params![
            p.id,
            p.provider,
            p.model,
            p.input_per_mtok,
            p.output_per_mtok
        ],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM pricing WHERE id=?1", params![id])?;
    Ok(())
}

/// Monthly budget per provider.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BudgetRow {
    pub id: String,
    pub provider: String,
    pub month: String,
    pub limit_usd: f64,
}

pub fn get_budgets(conn: &Connection) -> Result<Vec<BudgetRow>> {
    let mut stmt = conn
        .prepare("SELECT id, provider, month, limit_usd FROM budgets ORDER BY provider, month")?;
    let rows = stmt.query_map([], |row| {
        Ok(BudgetRow {
            id: row.get(0)?,
            provider: row.get(1)?,
            month: row.get(2)?,
            limit_usd: row.get(3)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

pub fn upsert_budget(conn: &Connection, b: &BudgetRow) -> Result<()> {
    conn.execute(
        "INSERT INTO budgets (id, provider, month, limit_usd)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(provider, month) DO UPDATE SET limit_usd=?4",
        params![b.id, b.provider, b.month, b.limit_usd],
    )?;
    Ok(())
}

pub fn delete_budget(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM budgets WHERE id=?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrations::run_migrations(&c).unwrap();
        c
    }

    #[test]
    fn test_pricing_crud() {
        let c = conn();
        upsert(
            &c,
            &PricingRow {
                id: "p1".into(),
                provider: "openai".into(),
                model: "gpt-4o".into(),
                input_per_mtok: 2.5,
                output_per_mtok: 10.0,
            },
        )
        .unwrap();
        // upsert same provider+model → updates, not duplicates
        upsert(
            &c,
            &PricingRow {
                id: "p1".into(),
                provider: "openai".into(),
                model: "gpt-4o".into(),
                input_per_mtok: 5.0,
                output_per_mtok: 15.0,
            },
        )
        .unwrap();
        let all = get_all(&c).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].input_per_mtok, 5.0);
        delete(&c, "p1").unwrap();
        assert!(get_all(&c).unwrap().is_empty());
    }

    #[test]
    fn test_budget_crud() {
        let c = conn();
        upsert_budget(
            &c,
            &BudgetRow {
                id: "b1".into(),
                provider: "openai".into(),
                month: "2026-08".into(),
                limit_usd: 50.0,
            },
        )
        .unwrap();
        upsert_budget(
            &c,
            &BudgetRow {
                id: "b1".into(),
                provider: "openai".into(),
                month: "2026-08".into(),
                limit_usd: 75.0,
            },
        )
        .unwrap();
        let all = get_budgets(&c).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].limit_usd, 75.0);
        delete_budget(&c, "b1").unwrap();
        assert!(get_budgets(&c).unwrap().is_empty());
    }
}
