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
use std::collections::HashMap;

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

/// GET /admin/free-providers?category=&configuredOnly= — curated free-tier
/// catalog with installed flag + telemetry ranking.
pub async fn list_free_providers(
    State(state): State<crate::proxy::AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let category = params.get("category").map(|s| s.as_str());
    let configured_only = params
        .get("configuredOnly")
        .is_some_and(|v| v == "1" || v == "true" || v == "yes");

    let rows = with_db(&state, |conn| {
        Ok(crate::free_providers::list_with_telemetry(
            Some(conn),
            category,
            configured_only,
        ))
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(json!({ "object": "list", "data": rows })))
}

/// POST /admin/free-providers/{id}/add — one-click add a free provider.
/// Body: { "api_key": "..." } (wajib untuk kategori apikey; kosong utk noauth).
pub async fn add_free_provider(
    State(state): State<crate::proxy::AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, String)> {
    let fp = crate::free_providers::get(&id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("free provider '{id}' tidak dikenal"),
        )
    })?;

    if fp.category == "apikey" && body["api_key"].as_str().unwrap_or("").is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "api_key wajib diisi untuk provider kategori apikey".to_string(),
        ));
    }

    let api_key = body["api_key"].as_str().map(String::from);
    let name = body["name"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| fp.name.clone());
    let now = chrono::Utc::now().to_rfc3339();

    let conn_row = omniroute_db::models::ProviderConnection {
        id: uuid::Uuid::new_v4().to_string(),
        provider: fp.provider.clone(),
        auth_type: Some("api".into()),
        name: Some(name),
        email: None,
        api_key,
        is_active: true,
        priority: Some(1),
        data: serde_json::json!({
            "format": fp.format,
            "base_url": fp.base_url,
            "free_provider": fp.id,
        }),
        rate_limited_until: None,
        backoff_level: None,
        created_at: now.clone(),
        updated_at: now,
    };

    with_db(&state, |conn| {
        omniroute_db::repos::provider_connection_repo::insert(conn, &conn_row)
            .map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    if let Some(db) = &state.db {
        crate::events::Events::audit(
            db,
            "create",
            "free_provider",
            Some(&conn_row.id),
            Some(&fp.id),
        );
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": conn_row.id, "provider": fp.provider, "name": fp.name })),
    ))
}

/// Synced (live API) models utk provider dari DB — empty kalau DB off.
fn synced_models(state: &crate::proxy::AppState, provider: &str) -> Vec<(String, Option<String>)> {
    let Some(db) = &state.db else {
        return Vec::new();
    };
    let Ok(conn) = db.conn.lock() else {
        return Vec::new();
    };
    omniroute_db::repos::synced_models_repo::list_for_provider(&conn, provider).unwrap_or_default()
}

/// GET /admin/models?provider=&q=&limit= — model list dari registry
/// (parity OmniRoute provider detail: getModelsByProviderId).
pub async fn list_models(
    State(state): State<crate::proxy::AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let provider = q.get("provider").cloned();
    let search = q.get("q").map(|s| s.to_lowercase()).unwrap_or_default();
    let limit: usize = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(1000);

    let mut out = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 1. Synced (live API) models dulu — preferred (parity OmniRoute:
    //    "Prefer synced API-discovered models when available")
    if let Some(pid) = &provider {
        for (id, name) in synced_models(&state, pid) {
            if !search.is_empty() && !id.to_lowercase().contains(&search) {
                continue;
            }
            out.push(json!({ "id": id, "name": name, "provider": pid, "synced": true }));
            seen.insert(id.clone());
            if out.len() >= limit {
                break;
            }
        }
    }

    // 2. Registry (built-in curated catalog) — skip yang sudah ada
    //    dari sync, tandai context_length/supports_reasoning.
    if out.len() < limit {
        'outer: for p in omniroute_providers::PROVIDER_LIST.iter() {
            if provider.as_ref().is_some_and(|pid| p.id != *pid) {
                continue;
            }
            for m in &p.models {
                if !search.is_empty() && !m.id.to_lowercase().contains(&search) {
                    continue;
                }
                if seen.contains(&m.id) {
                    continue;
                }
                out.push(json!({
                    "id": m.id,
                    "name": m.name,
                    "provider": p.id,
                    "context_length": m.context_length,
                    "supports_reasoning": m.supports_reasoning,
                    "synced": false,
                }));
                seen.insert(m.id.clone());
                if out.len() >= limit {
                    break 'outer;
                }
            }
        }
    }
    Ok(Json(
        json!({ "object": "list", "data": out, "total": out.len() }),
    ))
}

/// POST /admin/models/sync — fetch live model list dari modelsUrl provider
/// (parity OmniRoute modelsUrl/passthroughModels). Body: { provider }.
pub async fn sync_provider_models(
    State(state): State<crate::proxy::AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let provider = body["provider"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "provider wajib".to_string()))?;
    let reg = omniroute_providers::get_provider(provider).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("provider tidak dikenal: {provider}"),
        )
    })?;
    let url = reg
        .models_url
        .as_ref()
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                format!("{provider} tidak punya models_url (tidak support live sync)"),
            )
        })?
        .clone();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("fetch {url} gagal: {e}")))?;
    if !resp.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!("fetch {url} → HTTP {}", resp.status()),
        ));
    }
    let text = resp
        .text()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("baca body gagal: {e}")))?;
    let parsed: Value = serde_json::from_str(&text)
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("response bukan JSON: {e}")))?;

    let models = parsed
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                "response tidak punya field data[]".to_string(),
            )
        })?;
    let mut rows: Vec<(String, Option<String>)> = Vec::new();
    for m in models {
        if let Some(id) = m.get("id").and_then(|v| v.as_str()) {
            rows.push((
                id.to_string(),
                m.get("name").and_then(|v| v.as_str()).map(String::from),
            ));
        }
    }
    if rows.is_empty() {
        return Err((
            StatusCode::BAD_GATEWAY,
            "sync mengembalikan 0 model".to_string(),
        ));
    }

    let count = if let Some(db) = &state.db {
        if let Ok(conn) = db.conn.lock() {
            omniroute_db::repos::synced_models_repo::upsert_many(&conn, provider, &rows)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        } else {
            0
        }
    } else {
        0
    };

    if let Some(db) = &state.db {
        crate::events::Events::audit(
            db,
            "sync",
            "models",
            Some(provider),
            Some(&format!("{count} model dari {url}")),
        );
    }

    Ok(Json(json!({
        "ok": true,
        "provider": provider,
        "synced": count,
        "url": url,
        "total": rows.len(),
    })))
}

/// POST /admin/providers/test — kirim request chat kecil untuk verifikasi
/// key + koneksi sebelum disimpan. Body: { provider, api_key, base_url?, model? }.
pub async fn test_provider_connection(
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let provider = body["provider"]
        .as_str()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "provider wajib".to_string()))?;
    // api_key opsional — provider noauth (opencode) jalan tanpa key.
    let api_key = body["api_key"].as_str().unwrap_or("").to_string();
    let base_override = body["base_url"]
        .as_str()
        .map(String::from)
        .or_else(|| crate::config::base_urls_from_env().get(provider).cloned());

    let model = body["model"].as_str().map(String::from).unwrap_or_else(|| {
        omniroute_providers::get_provider(provider)
            .and_then(|p| p.models.first())
            .map(|m| m.id.clone())
            .unwrap_or_else(|| "gpt-4o-mini".to_string())
    });

    let executor = crate::executor::ProviderExecutor::from_provider_id_with_base(
        provider,
        &api_key,
        base_override.as_deref(),
    )
    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    let req = crate::chat::ChatRequest {
        model: model.clone(),
        messages: vec![crate::chat::Message {
            role: "user".into(),
            content: Some(crate::chat::Content::Text("ping".into())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        stream: Some(false),
        max_tokens: Some(4),
        temperature: None,
        top_p: None,
        stop: None,
        tools: None,
        tool_choice: None,
        extra: None,
        cache: false,
        cache_ttl: None,
        compress: false,
        max_context_tokens: None,
    };

    let start = std::time::Instant::now();
    match executor.execute_chat(&req).await {
        Ok(resp) => {
            let reply = resp
                .choices
                .first()
                .and_then(|c| c.message.content.as_ref())
                .map(|c| match c {
                    crate::chat::Content::Text(s) => s.clone(),
                    _ => "(non-text)".to_string(),
                })
                .unwrap_or_default();
            Ok(Json(json!({
                "ok": true,
                "provider": provider,
                "model": model,
                "latency_ms": start.elapsed().as_millis(),
                "reply": reply,
            })))
        }
        Err(e) => Ok(Json(json!({
            "ok": false,
            "provider": provider,
            "model": model,
            "latency_ms": start.elapsed().as_millis(),
            "error": e.to_string(),
        }))),
    }
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
        .route("/providers/test", post(test_provider_connection))
        .route("/models", get(list_models))
        .route("/models/sync", post(sync_provider_models))
        .route("/providers/{id}", put(update_provider_connection))
        .route("/providers/{id}", delete(delete_provider_connection))
        .route("/free-providers", get(list_free_providers))
        .route("/free-providers/{id}/add", post(add_free_provider))
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
        .route("/settings", get(handle_settings))
        .route("/cache", get(list_cache))
        .route("/cache", delete(clear_cache))
        .route("/cache/{key}", delete(delete_cache_entry))
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

// ── M5: Cache management ──────────────────────────────────────────────

/// GET /admin/cache — stats + entries.
pub async fn list_cache(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let items = with_db(&state, |c| {
        // purge expired first so stats are honest
        let _ = omniroute_db::repos::cache_repo::purge_expired(c);
        let s = omniroute_db::repos::cache_repo::stats(c).map_err(|e| e.to_string())?;
        let mut stmt = c
            .prepare("SELECT key, model, hits, created_at, expires_at FROM cache_entries ORDER BY created_at DESC LIMIT 200")
            .map_err(|e| e.to_string())?;
        let entries: Vec<Value> = stmt
            .query_map([], |row| {
                Ok(json!({
                    "key": row.get::<_, String>(0)?,
                    "model": row.get::<_, String>(1)?,
                    "hits": row.get::<_, i64>(2)?,
                    "created_at": row.get::<_, String>(3)?,
                    "expires_at": row.get::<_, String>(4)?,
                }))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        let mut out = s;
        out["entries_list"] = json!(entries);
        Ok(out)
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(items))
}

/// DELETE /admin/cache — flush all entries.
pub async fn clear_cache(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    with_db(&state, |c| {
        omniroute_db::repos::cache_repo::clear(c).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "ok": true })))
}

/// DELETE /admin/cache/{key} — remove one entry.
pub async fn delete_cache_entry(
    State(state): State<crate::proxy::AppState>,
    Path(key): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    with_db(&state, |c| {
        omniroute_db::repos::cache_repo::delete(c, &key).map_err(|e| e.to_string())
    })
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "ok": true, "key": key })))
}

/// GET /admin/settings — runtime configuration summary (no secrets).
pub async fn handle_settings(
    State(state): State<crate::proxy::AppState>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let providers = omniroute_providers::list_providers();
    let model_count: usize = providers.iter().map(|p| p.models.len()).sum();
    let mut features = vec![
        "routing".to_string(),
        "fallback".to_string(),
        "auto-combo-scoring".to_string(),
        "session-affinity".to_string(),
        "streaming".to_string(),
        "cache".to_string(),
        "telemetry".to_string(),
        "costs".to_string(),
        "webhooks".to_string(),
        "audit".to_string(),
        "mcp".to_string(),
        "a2a".to_string(),
        "batch".to_string(),
        "relay".to_string(),
        "compression".to_string(),
    ];
    if state.db.is_some() {
        features.push("sqlite".to_string());
    }
    Ok(Json(json!({
        "version": state.version,
        "started_at": state.started_at.to_rfc3339(),
        "uptime_seconds": (chrono::Utc::now() - state.started_at).num_seconds(),
        "port": 20129,
        "db_path": state.db_path,
        "db_connected": state.db.is_some(),
        "providers_registry": providers.len(),
        "models_registry": model_count,
        "features": features,
        "env": {
            "OMNIROUTE_PORT": "20129 (env)",
            "OMNIROUTE_DB_PATH": state.db_path,
            "OMNIROUTE_ADMIN_KEYS": "configured (masked)",
            "OMNIROUTE_API_KEYS": "configured (masked)",
            "OMNIROUTE_ALLOWED_HOSTS": "configured",
        },
    })))
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
