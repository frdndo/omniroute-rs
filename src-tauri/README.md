# M0 — Tauri Desktop Shell

Desktop wrapper for omniroute-rs: webview loads the React dashboard, the
Rust shell spawns the proxy (sidecar) on `127.0.0.1:20129`, and the
dashboard talks to the proxy via `VITE_PROXY_BASE` (set at build time).

> ⚠️ **Build wajib di macOS** — Linux server tidak punya toolchain
> wkwebview bundling (dan deps webkit2gtk dipasang hanya untuk `cargo
> check`). Tauri crate ini sengaja DI LUAR workspace `rust-core/` supaya
> CI server tidak menyentuhnya.

## Arsitektur

```
omniroute-rs.app (macOS)
├── WebView ──→ dashboard/dist (frontendDist)
├── Rust shell (src/lib.rs)
│   ├── spawn sidecar: binaries/omniroute-server (port 20129)
│   └── IPC: proxy_status / proxy_start / proxy_stop
└── Sidecar: server binary (release build rust-core, target macOS)
```

## Build di macOS

### 1. Prasyarat (sekali)

```bash
xcode-select --install                          # Xcode Command Line Tools
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust
brew install node                               # Node (untuk dashboard)
rustup target add aarch64-apple-darwin          # kalau Apple Silicon
```

### 2. Build sidecar proxy (binary utama)

```bash
cd rust-core
CARGO_BUILD_JOBS=1 cargo build --release -p omniroute-core --bin server
cp target/release/server ../src-tauri/binaries/omniroute-server
# Tauri butuh suffix target triple, contoh:
#   Apple Silicon: cp target/release/server ../src-tauri/binaries/omniroute-server-aarch64-apple-darwin
#   Intel:         cp target/release/server ../src-tauri/binaries/omniroute-server-x86_64-apple-darwin
```

### 3. Build app

```bash
cd src-tauri
cargo install tauri-cli --locked     # crate tauri-cli, binary cargo-tauri
cargo tauri icon <logo-1024.png>           # ganti placeholder icon (wajib utk .icns) — bukan npm run!
cargo tauri build
# → target/release/bundle/macos/omniroute-rs.app
# → target/release/bundle/macos/omniroute-rs_0.1.0_aarch64.dmg
```

### 4. Dev mode (hot reload)

```bash
cd src-tauri && cargo tauri dev
# beforeDevCommand menjalankan dashboard dev server (vite :5173)
```

## Catatan

- `beforeBuildCommand` membangun dashboard dengan
  `VITE_PROXY_BASE=http://127.0.0.1:20129` supaya fetch `/admin/*` dan
  `/v1/*` mengarah ke proxy sidecar (bukan origin tauri://).
- Proxy auto-start di `setup()`; port default 20129; DB default
  `data/omniroute.db` (relatif ke working dir app).
- Env `OMNIROUTE_*` dari shell induk tetap diteruskan — override
  `OMNIROUTE_PORT`, `OMNIROUTE_ADMIN_KEYS`, dll sesuai kebutuhan.
- Icons placeholder (PNG) di-commit; `.icns`/`.ico` dihasilkan saat
  `cargo tauri icon` di macOS.
