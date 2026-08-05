use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_shell::ShellExt;

/// M0 Tauri shell: spawns the omniroute-rs proxy (sidecar binary) on port
/// 20129 and exposes IPC commands to start/stop/inspect it. The webview
/// loads the React dashboard (frontendDist) which talks to the proxy over
/// http://127.0.0.1:20129 (VITE_PROXY_BASE set at build time).

struct ProxyState {
    child: Mutex<Option<tauri_plugin_shell::process::CommandChild>>,
    port: u16,
}

impl Default for ProxyState {
    fn default() -> Self {
        Self {
            child: Mutex::new(None),
            port: 20129,
        }
    }
}

#[derive(Serialize)]
struct ProxyStatus {
    running: bool,
    port: u16,
    version: String,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Absolute SQLite path inside the app data dir (bundled .app runs with
/// CWD=/ — a relative path like data/omniroute.db would fail to write).
fn proxy_db_path(app: &tauri::AppHandle) -> String {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default());
    let _ = std::fs::create_dir_all(&dir);
    dir.join("omniroute.db").to_string_lossy().into_owned()
}

fn sidecar_name() -> String {
    format!("omniroute-server-{}", env!("TAURI_ENV_TARGET_TRIPLE"))
}

fn spawn_proxy(
    app: &tauri::AppHandle,
    port: u16,
) -> Result<tauri_plugin_shell::process::CommandChild, String> {
    let db_path = proxy_db_path(app);
    let name = sidecar_name();

    // Resolve path sidecar MANUAL — shell.sidecar() resolve-nya beda di
    // dev (target/debug/binaries/...) vs lokasi file asli
    // (src-tauri/binaries/...) → ENOENT "No such file or directory".
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join("binaries").join(&name)); // prod (bundled)
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // dev: exe = src-tauri/target/debug/omniroute-rs → ../../binaries
            candidates.push(dir.join("..").join("..").join("binaries").join(&name));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("binaries").join(&name)); // dev fallback
    }

    let bin = candidates
        .iter()
        .find(|p| p.exists())
        .ok_or_else(|| {
            let tried = candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            format!("sidecar '{name}' not found (tried: {tried})")
        })?;

    let shell = app.shell();
    let mut command = shell.command(bin.clone());
    command = command
        .env("OMNIROUTE_PORT", port.to_string())
        .env("OMNIROUTE_DB_PATH", db_path);

    // Dev convenience: proxy fail-closed tanpa ADMIN_KEYS (semua /admin
    // → 503). Di DEBUG build, isi default kalau env tidak diset supaya
    // login langsung jalan. Release build tetap strict (tanpa default).
    #[cfg(debug_assertions)]
    {
        if std::env::var("OMNIROUTE_ADMIN_KEYS").is_err() {
            command = command.env("OMNIROUTE_ADMIN_KEYS", "sk-admin");
        }
        if std::env::var("OMNIROUTE_API_KEYS").is_err() {
            command = command.env("OMNIROUTE_API_KEYS", "sk-gateway");
        }
        if std::env::var("OMNIROUTE_ALLOWED_HOSTS").is_err() {
            command = command.env("OMNIROUTE_ALLOWED_HOSTS", "localhost,127.0.0.1");
        }
    }

    let (_events, child) = command
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;
    Ok(child)
}

#[tauri::command]
fn proxy_status(state: State<ProxyState>) -> ProxyStatus {
    let running = state.child.lock().map(|c| c.is_some()).unwrap_or(false);
    ProxyStatus {
        running,
        port: state.port,
        version: VERSION.to_string(),
    }
}

/// Start the proxy sidecar with sane defaults. Env can be overridden via
/// the `OMNIROUTE_*` variables (already exported in the parent process).
#[tauri::command]
async fn proxy_start(app: tauri::AppHandle, state: State<'_, ProxyState>) -> Result<ProxyStatus, String> {
    {
        let mut guard = state.child.lock().map_err(|_| "state poisoned".to_string())?;
        if guard.is_some() {
            return Ok(ProxyStatus {
                running: true,
                port: state.port,
                version: VERSION.to_string(),
            });
        }
        if let Ok(child) = spawn_proxy(&app, state.port) {
            *guard = Some(child);
        }
    }

    // Wait for the proxy to be reachable (up to 5s).
    for _ in 0..25 {
        if let Ok(resp) = reqwest_lite_ping(state.port).await {
            if resp {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    Ok(ProxyStatus {
        running: true,
        port: state.port,
        version: VERSION.to_string(),
    })
}

#[tauri::command]
async fn proxy_stop(state: State<'_, ProxyState>) -> Result<(), String> {
    let mut guard = state.child.lock().map_err(|_| "state poisoned".to_string())?;
    if let Some(child) = guard.take() {
        let _ = child.kill();
    }
    Ok(())
}

/// Minimal readiness probe: TCP connect to the proxy port.
async fn reqwest_lite_ping(port: u16) -> Result<bool, ()> {
    tauri::async_runtime::spawn_blocking(move || {
        std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(300),
        )
        .is_ok()
    })
    .await
    .map_err(|_| ())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(ProxyState::default())
        .invoke_handler(tauri::generate_handler![
            proxy_status,
            proxy_start,
            proxy_stop
        ])
        .setup(|app| {
            // Auto-start the proxy when the app launches.
            let handle = app.handle().clone();
            let port = 20129;
            tauri::async_runtime::spawn(async move {
                // sidecar auto-start; events channel tersedia untuk M3
                // (log streaming) via tauri_plugin_shell CommandEvent.
                match spawn_proxy(&handle, port) {
                    Ok(_) => eprintln!("[omniroute-rs] proxy sidecar started on port {port}"),
                    Err(e) => eprintln!("[omniroute-rs] proxy sidecar FAILED to start: {e}"),
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
