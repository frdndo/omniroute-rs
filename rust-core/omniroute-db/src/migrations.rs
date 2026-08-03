use rusqlite::Connection;

pub fn run_migrations(conn: &Connection) -> Result<(), anyhow::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS _migrations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS providerConnections (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            auth_type TEXT,
            name TEXT,
            email TEXT,
            api_key TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            priority INTEGER DEFAULT 999,
            data TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS apiKeys (
            id TEXT PRIMARY KEY,
            key TEXT NOT NULL UNIQUE,
            name TEXT,
            machine_id TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS combos (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            kind TEXT NOT NULL DEFAULT 'model',
            models TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        ",
    )?;

    // Migration v2: health tracking fields (match OmniRoute provider_connections)
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(providerConnections)")?
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<_, _>>()?;
    if !cols.iter().any(|c| c == "rate_limited_until") {
        conn.execute(
            "ALTER TABLE providerConnections ADD COLUMN rate_limited_until TEXT",
            [],
        )?;
    }
    if !cols.iter().any(|c| c == "backoff_level") {
        conn.execute(
            "ALTER TABLE providerConnections ADD COLUMN backoff_level INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    // Migration v3: session → account affinity (multi-turn stickiness)
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS session_account_affinity (
            session_id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            account_key TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        ",
    )?;

    // Migration v4: per-provider runtime stats for the auto-combo scorer
    // (survive restarts so scoring doesn't relearn from zero)
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS provider_stats (
            provider TEXT PRIMARY KEY,
            latency_ema_ms REAL NOT NULL DEFAULT 0,
            total_requests INTEGER NOT NULL DEFAULT 0,
            failed_requests INTEGER NOT NULL DEFAULT 0
        );
        ",
    )?;

    // Migration v5: persistent request telemetry (analytics foundation)
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS request_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            method TEXT NOT NULL,
            uri TEXT NOT NULL,
            status INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            provider TEXT,
            model TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_request_logs_ts ON request_logs(ts);
        ",
    )?;

    Ok(())
}
