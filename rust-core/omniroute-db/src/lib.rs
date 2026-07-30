pub mod migrations;
pub mod models;
pub mod repos;

pub use models::*;

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use tracing::info;

pub struct Database {
    pub conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, anyhow::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        migrations::run_migrations(&conn)?;
        info!("Database opened: {}", path.display());
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
    pub fn open_in_memory() -> Result<Self, anyhow::Error> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        migrations::run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}
