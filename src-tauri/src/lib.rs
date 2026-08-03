use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::State;
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
        let shell = app.shell();
        let (_events, child) = shell
            .command("binaries/omniroute-server")
            .env("OMNIROUTE_PORT", state.port.to_string())
            .env("OMNIROUTE_DB_PATH", "data/omniroute.db")
            .spawn()
            .map_err(|e| format!("spawn failed: {e}"))?;
        *guard = Some(child);
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
                let shell = handle.shell();
                let result = shell
                    .command("binaries/omniroute-server")
                    .env("OMNIROUTE_PORT", port.to_string())
                    .env("OMNIROUTE_DB_PATH", "data/omniroute.db")
                    .spawn();
                if let Ok((_events, _child)) = result {
                    // sidecar running; events channel can be used later
                    // for log streaming (M3 systray)
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
