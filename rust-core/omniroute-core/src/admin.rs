use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post, put},
};
use omniroute_db::{
    models::{ApiKey, ProviderConnection},
    repos::api_key_repo,
    repos::provider_connection_repo,
};
use serde_json::{Value, json};

/// Admin API keys from `OMNIROUTE_ADMIN_KEYS` (comma-separated).
/// If empty → admin endpoints are DISABLED (fail closed).
#[derive(Debug, Default, Clone)]
pub struct AdminKeys {
    keys: Vec<String>,
}

impl AdminKeys {
    pub fn new(keys: Vec<String>) -> Self {
        Self { keys }
    }

    pub fn from_env() -> Self {
        std::env::var("OMNIROUTE_ADMIN_KEYS")
            .map(|v| {
                Self::new(
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect(),
                )
            })
            .unwrap_or_default()
    }

    pub fn enabled(&self) -> bool {
        !self.keys.is_empty()
    }

    pub fn validate(&self, token: Option<&str>) -> bool {
        match token {
            Some(t) => self.keys.iter().any(|k| k == t),
            None => false,
        }
    }
}

/// Admin auth middleware — fail closed when no admin keys configured.
pub async fn admin_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let keys = request
        .extensions()
        .get::<AdminKeys>()
        .cloned()
        .unwrap_or_default();

    if !keys.enabled() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    let token = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string());

    if keys.validate(token.as_deref()) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Mask an API key: keep first 4 + last 4 chars.
fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}****{}", &key[..4], &key[key.len() - 4..])
}

fn with_db<T>(
    state: &crate::proxy::AppState,
    f: impl FnOnce(&rusqlite::Connection) -> Result<T, String>,
) -> Result<T, String> {
    // Prefer the shared DB handle (opened once at startup)
    if let Some(db) = &state.db {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        return f(&conn);
    }
    // Fallback: open a fresh connection (tests, admin-only mode)
    let db = omniroute_db::Database::open(std::path::Path::new(&state.db_path))
        .map_err(|e| e.to_string())?;
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    f(&conn)
}

// ── Handlers ─────────────────────────────────────────────────────────

pub async fn list_provider_connections(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = with_db(&state, |conn| {
        provider_connection_repo::get_all(conn).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // Redact API keys before returning
    let redacted: Vec<Value> = items
        .into_iter()
        .map(|c| {
            let mut v = serde_json::to_value(&c).unwrap_or_default();
            if let Some(key) = v["api_key"].as_str() {
                v["api_key"] = json!(mask_key(key));
            }
            v
        })
        .collect();

    Ok(Json(json!({ "object": "list", "data": redacted })))
}

pub async fn create_provider_connection(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, String)> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let conn = ProviderConnection {
        id: id.clone(),
        provider: body["provider"].as_str().unwrap_or("").to_string(),
        auth_type: body["auth_type"].as_str().map(String::from),
        name: body["name"].as_str().map(String::from),
        email: body["email"].as_str().map(String::from),
        api_key: body["api_key"].as_str().map(String::from),
        is_active: body["is_active"].as_bool().unwrap_or(true),
        priority: body["priority"].as_i64().map(|i| i as i32),
        data: body["data"].clone(),
        rate_limited_until: body["rate_limited_until"].as_str().map(String::from),
        backoff_level: body["backoff_level"].as_i64().map(|i| i as i32),
        created_at: now.clone(),
        updated_at: now,
    };
    if conn.provider.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "provider is required".into()));
    }

    with_db(&state, |c| {
        provider_connection_repo::insert(c, &conn).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // Pick up the new connection in routing immediately
    state.reload_accounts();
    if let Some(db) = &state.db {
        crate::events::Events::audit(db, "create", "provider", Some(&id), Some(&conn.provider));
    }

    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn update_provider_connection(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let mut item = with_db(&state, |c| {
        provider_connection_repo::get_by_id(c, &id).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
    .ok_or((
        StatusCode::NOT_FOUND,
        format!("connection {} not found", id),
    ))?;

    if let Some(p) = body["provider"].as_str() {
        item.provider = p.to_string();
    }
    if let Some(k) = body["api_key"].as_str() {
        item.api_key = Some(k.to_string());
    }
    if let Some(a) = body["is_active"].as_bool() {
        item.is_active = a;
    }
    item.updated_at = chrono::Utc::now().to_rfc3339();

    with_db(&state, |c| {
        provider_connection_repo::update(c, &item).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    state.reload_accounts();
    Ok(Json(json!({ "ok": true, "id": id })))
}

pub async fn delete_provider_connection(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    with_db(&state, |c| {
        provider_connection_repo::delete(c, &id).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    state.reload_accounts();
    if let Some(db) = &state.db {
        crate::events::Events::audit(db, "delete", "provider", Some(&id), None);
    }
    Ok(Json(json!({ "ok": true, "id": id })))
}

pub async fn list_api_keys(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = with_db(&state, |c| {
        api_key_repo::get_all(c).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let redacted: Vec<Value> = items
        .into_iter()
        .map(|k| {
            let mut v = serde_json::to_value(&k).unwrap_or_default();
            v["key"] = json!(mask_key(&k.key));
            v
        })
        .collect();

    Ok(Json(json!({ "object": "list", "data": redacted })))
}

pub async fn create_api_key(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, String)> {
    let key = body["key"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| format!("sk-{}", uuid::Uuid::new_v4().to_string().replace('-', "")));
    let now = chrono::Utc::now().to_rfc3339();
    let item = ApiKey {
        id: uuid::Uuid::new_v4().to_string(),
        key: key.clone(),
        name: body["name"].as_str().map(String::from),
        machine_id: body["machine_id"].as_str().map(String::from),
        is_active: true,
        created_at: now,
    };

    with_db(&state, |c| {
        api_key_repo::insert(c, &item).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    // New gateway key applies immediately
    state.reload_accounts();

    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": item.id, "key": key })),
    ))
}

pub async fn delete_api_key(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    with_db(&state, |c| {
        api_key_repo::delete(c, &id).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    state.reload_accounts();
    Ok(Json(json!({ "ok": true, "id": id })))
}

pub async fn update_api_key(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let name = body.get("name").and_then(|v| v.as_str());
    let is_active = body.get("is_active").and_then(|v| v.as_bool());
    if name.is_none() && is_active.is_none() {
        return Err((StatusCode::BAD_REQUEST, "name or is_active required".into()));
    }
    with_db(&state, |c| {
        api_key_repo::update(c, &id, name, is_active).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    // is_active change affects gateway auth immediately
    state.reload_accounts();
    Ok(Json(json!({ "ok": true, "id": id })))
}

// ── Combo management (fallback chains) ───────────────────────────────

pub async fn list_combos(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = with_db(&state, |c| {
        omniroute_db::repos::combo_repo::get_all(c).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "object": "list", "data": items })))
}

pub async fn create_combo(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, String)> {
    let name = body["name"].as_str().unwrap_or("").to_string();
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name is required".into()));
    }
    let models: Vec<String> = body["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if models.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "models[] must not be empty".into()));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let combo = omniroute_db::models::Combo {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        kind: body["kind"].as_str().unwrap_or("model").to_string(),
        models,
        created_at: now.clone(),
        updated_at: now,
    };

    with_db(&state, |c| {
        omniroute_db::repos::combo_repo::insert(c, &combo).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    state.reload_accounts();
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": combo.id, "name": combo.name })),
    ))
}

pub async fn delete_combo(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    with_db(&state, |c| {
        omniroute_db::repos::combo_repo::delete(c, &id).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    state.reload_accounts();
    Ok(Json(json!({ "ok": true, "id": id })))
}

/// Build the admin sub-router.
pub fn admin_router(state: crate::proxy::AppState) -> Router {
    Router::new()
        .route("/providers", get(list_provider_connections))
        .route("/providers", post(create_provider_connection))
        .route("/providers/{id}", put(update_provider_connection))
        .route("/providers/{id}", delete(delete_provider_connection))
        .route("/api-keys", get(list_api_keys))
        .route("/api-keys", post(create_api_key))
        .route("/api-keys/{id}", put(update_api_key))
        .route("/api-keys/{id}", delete(delete_api_key))
        .route("/combos", get(list_combos))
        .route("/combos", post(create_combo))
        .route("/combos/{id}", delete(delete_combo))
        .route("/logs", get(crate::logs::handle_logs))
        .route("/stats", get(handle_stats))
        .route("/pricing", get(list_pricing))
        .route("/pricing", post(upsert_pricing))
        .route("/pricing/{id}", delete(delete_pricing))
        .route("/budgets", get(list_budgets))
        .route("/budgets", post(upsert_budget))
        .route("/budgets/{id}", delete(delete_budget))
        .route("/costs", get(handle_costs))
        .route("/webhooks", get(list_webhooks))
        .route("/webhooks", post(create_webhook))
        .route("/webhooks/{id}", put(update_webhook))
        .route("/webhooks/{id}", delete(delete_webhook))
        .route("/audit", get(list_audit))
        .with_state(state)
}

/// GET /admin/stats — telemetry aggregates for the Analytics dashboard.
pub async fn handle_stats() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    Ok(Json(crate::telemetry::TELEMETRY.stats()))
}

// ── M3: Pricing & budgets & costs ─────────────────────────────────────

pub async fn list_pricing(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = with_db(&state, |c| {
        omniroute_db::repos::pricing_repo::get_all(c).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "object": "list", "data": items })))
}

pub async fn upsert_pricing(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, String)> {
    let provider = body["provider"].as_str().unwrap_or("").to_string();
    let model = body["model"].as_str().unwrap_or("*").to_string();
    if provider.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "provider required".into()));
    }
    let item = omniroute_db::repos::pricing_repo::PricingRow {
        id: body["id"].as_str().unwrap_or("").to_string(),
        provider,
        model,
        input_per_mtok: body["input_per_mtok"].as_f64().unwrap_or(0.0),
        output_per_mtok: body["output_per_mtok"].as_f64().unwrap_or(0.0),
    };
    let id = if item.id.is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        item.id.clone()
    };
    let item = omniroute_db::repos::pricing_repo::PricingRow {
        id: id.clone(),
        ..item
    };
    with_db(&state, |c| {
        omniroute_db::repos::pricing_repo::upsert(c, &item).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok((StatusCode::CREATED, Json(json!({ "ok": true, "id": id }))))
}

pub async fn delete_pricing(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    with_db(&state, |c| {
        omniroute_db::repos::pricing_repo::delete(c, &id).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

pub async fn list_budgets(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = with_db(&state, |c| {
        omniroute_db::repos::pricing_repo::get_budgets(c).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "object": "list", "data": items })))
}

pub async fn upsert_budget(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, String)> {
    let provider = body["provider"].as_str().unwrap_or("").to_string();
    let month = body["month"].as_str().unwrap_or("").to_string();
    if provider.is_empty() || month.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "provider and month required".into(),
        ));
    }
    let item = omniroute_db::repos::pricing_repo::BudgetRow {
        id: uuid::Uuid::new_v4().to_string(),
        provider,
        month,
        limit_usd: body["limit_usd"].as_f64().unwrap_or(0.0),
    };
    with_db(&state, |c| {
        omniroute_db::repos::pricing_repo::upsert_budget(c, &item).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "ok": true, "id": item.id })),
    ))
}

pub async fn delete_budget(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    with_db(&state, |c| {
        omniroute_db::repos::pricing_repo::delete_budget(c, &id).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

/// GET /admin/costs?month=YYYY-MM — spend + budget report.
pub async fn handle_costs(
    State(state): State<crate::proxy::AppState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let month = query
        .get("month")
        .cloned()
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m").to_string());
    let Some(db) = &state.db else {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "no database".into()));
    };
    Ok(Json(crate::costs::Costs::report(db, &month)))
}

// ── M4: Webhooks & audit ──────────────────────────────────────────────

pub async fn list_webhooks(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = with_db(&state, |c| {
        omniroute_db::repos::webhook_repo::get_all(c).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "object": "list", "data": items })))
}

pub async fn create_webhook(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, String)> {
    let name = body["name"].as_str().unwrap_or("").to_string();
    let url = body["url"].as_str().unwrap_or("").to_string();
    if name.is_empty() || url.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name and url required".into()));
    }
    let events = body["events"]
        .as_str()
        .unwrap_or("chat.success,chat.error")
        .to_string();
    let w = omniroute_db::repos::webhook_repo::WebhookRow {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        url,
        events,
        is_active: body["is_active"].as_bool().unwrap_or(true),
    };
    with_db(&state, |c| {
        omniroute_db::repos::webhook_repo::insert(c, &w).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if let Some(db) = &state.db {
        crate::events::Events::audit(db, "create", "webhook", Some(&w.id), Some(&w.name));
    }
    Ok((StatusCode::CREATED, Json(json!({ "id": w.id }))))
}

pub async fn update_webhook(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let name = body["name"].as_str().unwrap_or("").to_string();
    let url = body["url"].as_str().unwrap_or("").to_string();
    let events = body["events"].as_str().unwrap_or("").to_string();
    if name.is_empty() || url.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "name and url required".into()));
    }
    let w = omniroute_db::repos::webhook_repo::WebhookRow {
        id: id.clone(),
        name,
        url,
        events,
        is_active: body["is_active"].as_bool().unwrap_or(true),
    };
    with_db(&state, |c| {
        omniroute_db::repos::webhook_repo::update(c, &id, &w).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

pub async fn delete_webhook(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    with_db(&state, |c| {
        omniroute_db::repos::webhook_repo::delete(c, &id).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

pub async fn list_audit(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = with_db(&state, |c| {
        let rows = omniroute_db::repos::webhook_repo::audit_recent(c, 200);
        serde_json::to_value(&rows).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "object": "list", "data": items })))
}

/// Build admin routes with auth applied, to be nested under /admin.
/// Extension layer goes OUTSIDE the middleware (set before it runs).
pub fn build_admin_router(admin_state: crate::proxy::AppState, admin_keys: AdminKeys) -> Router {
    admin_router(admin_state)
        .layer(middleware::from_fn(admin_middleware))
        .layer(axum::Extension(admin_keys))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_key() {
        assert_eq!(mask_key("sk-abcdefghijkl1234"), "sk-a****1234");
        assert_eq!(mask_key("short"), "****");
    }

    #[test]
    fn test_admin_keys_validate() {
        let keys = AdminKeys::new(vec!["admin-1".into()]);
        assert!(keys.enabled());
        assert!(keys.validate(Some("admin-1")));
        assert!(!keys.validate(Some("nope")));
        assert!(!keys.validate(None));
    }

    #[test]
    fn test_admin_keys_disabled_by_default() {
        let keys = AdminKeys::default();
        assert!(!keys.enabled());
        assert!(!keys.validate(Some("anything")));
    }
}
