pub mod a2a;
pub mod account;
pub mod admin;
pub mod auth;
pub mod auto_combo;
pub mod batch;
pub mod cache;
pub mod chat;
pub mod combo;
pub mod compress;
pub mod config;
pub mod costs;
pub mod events;
pub mod executor;
pub mod free_providers;
pub mod logs;
pub mod mcp;
pub mod proxy;
pub mod ratelimit;
pub mod router;
pub mod sanitize;
pub mod scorer;
pub mod sse;
pub mod telemetry;
pub mod translator;

use napi_derive::napi;
use omniroute_db::Database;
use once_cell::sync::OnceCell;
use tracing::info;

static DB: OnceCell<Database> = OnceCell::new();

#[napi]
pub fn init_database(path: String) -> napi::Result<()> {
    tracing_subscriber::fmt::init();
    let db = Database::open(std::path::Path::new(&path))
        .map_err(|e| napi::Error::from_reason(format!("DB init failed: {}", e)))?;
    DB.set(db)
        .map_err(|_| napi::Error::from_reason("DB already initialized"))?;
    info!("Database initialized: {}", path);
    Ok(())
}

#[napi]
pub fn get_provider_connections() -> napi::Result<Vec<serde_json::Value>> {
    let db = DB
        .get()
        .ok_or_else(|| napi::Error::from_reason("DB not initialized"))?;
    let conn = db
        .conn
        .lock()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let connections = omniroute_db::repos::provider_connection_repo::get_all(&conn)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(connections
        .into_iter()
        .map(|c| serde_json::to_value(c).unwrap())
        .collect())
}

#[napi]
pub fn create_provider_connection(data: serde_json::Value) -> napi::Result<()> {
    let connection: omniroute_db::ProviderConnection = serde_json::from_value(data)
        .map_err(|e| napi::Error::from_reason(format!("Invalid data: {}", e)))?;
    let db = DB
        .get()
        .ok_or_else(|| napi::Error::from_reason("DB not initialized"))?;
    let conn = db
        .conn
        .lock()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    omniroute_db::repos::provider_connection_repo::insert(&conn, &connection)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    info!("Created provider connection: {}", connection.id);
    Ok(())
}

#[napi]
pub fn update_provider_connection(id: String, data: serde_json::Value) -> napi::Result<()> {
    let mut connection: omniroute_db::ProviderConnection = serde_json::from_value(data)
        .map_err(|e| napi::Error::from_reason(format!("Invalid data: {}", e)))?;
    connection.id = id;
    let db = DB
        .get()
        .ok_or_else(|| napi::Error::from_reason("DB not initialized"))?;
    let conn = db
        .conn
        .lock()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    omniroute_db::repos::provider_connection_repo::update(&conn, &connection)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(())
}

#[napi]
pub fn delete_provider_connection(id: String) -> napi::Result<()> {
    let db = DB
        .get()
        .ok_or_else(|| napi::Error::from_reason("DB not initialized"))?;
    let conn = db
        .conn
        .lock()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    omniroute_db::repos::provider_connection_repo::delete(&conn, &id)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    info!("Deleted provider connection: {}", id);
    Ok(())
}

#[napi]
pub fn list_providers() -> napi::Result<Vec<serde_json::Value>> {
    let providers = omniroute_providers::list_providers();
    Ok(providers
        .into_iter()
        .map(|p| serde_json::to_value(p).unwrap())
        .collect())
}

#[napi]
pub fn get_provider(id: String) -> napi::Result<serde_json::Value> {
    let provider = omniroute_providers::get_provider(&id)
        .ok_or_else(|| napi::Error::from_reason(format!("Provider '{}' not found", id)))?;
    Ok(serde_json::to_value(provider).unwrap())
}

#[napi]
pub fn verify_api_key(key: String) -> napi::Result<bool> {
    let db = DB
        .get()
        .ok_or_else(|| napi::Error::from_reason("DB not initialized"))?;
    let conn = db
        .conn
        .lock()
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let keys = omniroute_db::repos::api_key_repo::get_all(&conn)
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(keys.iter().any(|k| k.key == key && k.is_active))
}
