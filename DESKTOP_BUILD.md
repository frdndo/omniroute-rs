# 🖥️ Desktop Build Guideline — omniroute-rs (M0 Tauri)

Panduan build aplikasi desktop **omniroute-rs** (Tauri v2) di **macOS**.

> ⚠️ Build **wajib di macOS** (Apple Silicon atau Intel). Server Linux tidak
> punya toolchain bundling wkwebview. Semua kode shell sudah siap di
> `src-tauri/` dan sudah diverifikasi `cargo check` (0 warning/error).

---

## 1. Arsitektur Target

```
omniroute-rs.app (macOS)
├── WebView ──→ dashboard (React, frontendDist: dashboard/dist)
├── Rust Tauri shell (src-tauri/src/lib.rs)
│   ├── auto-spawn sidecar → 127.0.0.1:20129
│   └── IPC commands: proxy_status / proxy_start / proxy_stop
└── Sidecar: binaries/omniroute-server-<triple>
    (release build rust-core, binary utama proxy)
```

- Dashboard fetch `/admin/*` + `/v1/*` ke proxy lokal via
  `VITE_PROXY_BASE=http://127.0.0.1:20129` (diset otomatis di
  `beforeBuildCommand`).
- DB default `data/omniroute.db` (relatif ke working dir app).
- Env `OMNIROUTE_*` dari shell induk tetap diteruskan (override).

---

## 2. Prasyarat (sekali saja)

```bash
# Xcode Command Line Tools
xcode-select --install

# Rust (rustup)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Node.js (buat dashboard)
brew install node

# Tauri CLI
cargo install cargo-tauri-cli --locked

# Dependensi dashboard (WAJIB sebelum build — beforeBuildCommand
# menjalankan npm run build di ../dashboard)
cd omniroute-rs/dashboard && npm install
```

Cek versi:
```bash
rustc --version   # ≥ 1.77
node --version    # ≥ 18
cargo tauri --version
```

---

## 3. Build Sidecar (binary proxy utama)

```bash
cd omniroute-rs/rust-core

# Build release (2C/2GB → wajib 1 job)
CARGO_BUILD_JOBS=1 cargo build --release -p omniroute-core --bin server

# Copy ke src-tauri/binaries DENGAN SUFFIX target triple:
#   Apple Silicon (M1/M2/M3/M4):
cp target/release/server ../src-tauri/binaries/omniroute-server-aarch64-apple-darwin
#   Intel Mac:
cp target/release/server ../src-tauri/binaries/omniroute-server-x86_64-apple-darwin
```

> Nama file harus persis `omniroute-server-<triple>` — Tauri externalBin
> mencari binary dengan suffix ini saat bundling.

---

## 4. Icons (ganti placeholder)

```bash
cd omniroute-rs/src-tauri

# Dari logo 1024x1024 PNG (transparan)
cargo tauri icon <logo-1024.png>

# Menghasilkan: icons/*.png, icons/icon.icns, icons/icon.ico
```

> ⚠️ JANGAN pakai `npm run tauri icon` — src-tauri TIDAK punya
> package.json (pure Rust crate), jadi npm run pasti error. Pakai
> `cargo tauri icon` saja.

Repo sudah berisi placeholder PNG (valid, bisa dibuild) — langkah ini
wajib hanya kalau mau icon asli.

---

## 5. Build Aplikasi

```bash
cd omniroute-rs/src-tauri

# Release build (dashboard dibuild otomatis dgn VITE_PROXY_BASE)
cargo tauri build

# Hasil:
#   target/release/bundle/macos/omniroute-rs.app
#   target/release/bundle/macos/omniroute-rs_0.1.0_aarch64.dmg
```

### Opsi build

| Perintah | Fungsi |
|---|---|
| `cargo tauri build` | .app + .dmg (release) |
| `cargo tauri build --debug` | App debug (tanpa bundle) |
| `cargo tauri dev` | Mode dev: vite hot-reload + shell, sidecar harus dijalankan manual dulu |

### Dev mode (hot reload)

> Sidecar TETAP perlu di-copy dulu (lihat §3) — `cargo tauri dev` juga
> me-resolve externalBin. Proxy di-auto-start oleh app sendiri, jadi
> TIDAK perlu menjalankan proxy manual (kalau dipaksa manual, port
> 20129 akan bentrok).

```bash
# 1. Copy sidecar dulu (lihat §3, pakai suffix triple macOS-mu)

# 2. Tauri dev (dashboard vite hot-reload di :5173)
cd src-tauri
cargo tauri dev
```

---

## 6. Env Variables yang Relevan

| Variable | Default | Fungsi |
|---|---|---|
| `OMNIROUTE_PORT` | 20129 | Port proxy (sidecar) |
| `OMNIROUTE_DB_PATH` | `~/Library/Application Support/com.frdndo.omniroute-rs/omniroute.db` (app_data_dir) | Path SQLite — **absolut**, sudah di-resolve shell |
| `OMNIROUTE_ADMIN_KEYS` | — (fail-closed 503) | Admin key dashboard |
| `OMNIROUTE_API_KEYS` | — | Gateway key (Playground, MCP, A2A) |
| `OMNIROUTE_PROVIDER_KEYS` | — | Fallback provider keys (format `provider=sk-...`) |
| `OMNIROUTE_BASE_URL_<PROVIDER>` | — | Override upstream (suffix `/v1`) |
| `OMNIROUTE_ALLOWED_HOSTS` | — | Host guard (403 kalau spoof) |
| `RUST_LOG` | info | Level log |

Di app desktop, env di-set lewat:
- **Terminal launch** (`./target/.../omniroute-rs` atau `open` dari terminal): env shell diteruskan.
- **Finder / dock launch**: shell env TIDAK ikut — set via
  `launchctl setenv OMNIROUTE_ADMIN_KEYS sk-admin` (sekali per boot)
  atau tulis di launchd plist. Default `OMNIROUTE_DB_PATH` sudah
  di-hardcode absolut oleh shell (app_data_dir), jadi DB tetap jalan.

---

## 7. Troubleshooting

| Masalah | Solusi |
|---|---|
| `resource path binaries/omniroute-server-<triple> doesn't exist` | Sidecar belum di-copy dengan suffix triple — lihat §3 |
| App "tak dikenal" / Gatekeeper | `xattr -dr com.apple.quarantine /path/omniroute-rs.app` (dev) atau signing resmi |
| Port 20129 sudah dipakai | Set `OMNIROUTE_PORT` lain + rebuild dashboard dengan base URL yang sama |
| `npm run tauri icon` error | JANGAN pakai ini — src-tauri tanpa package.json; pakai `cargo tauri icon` |
| DB `data/omniroute.db` gagal dibuat | Sudah diperbaiki: shell pakai app_data_dir absolut (`~/Library/Application Support/com.frdndo.omniroute-rs/`) |
| Build lambat | `CARGO_BUILD_JOBS=1` wajib di mesin 2C; pakai `--debug` untuk iterasi cepat |
| WebView blank (prod) | Pastikan `frontendDist` terisi — `cargo tauri build` membuild dashboard otomatis |

---

## 8. Checklist Sebelum Release

```
[ ] Sidecar built (release) + suffix triple benar
[ ] Icon asli (cargo tauri icon)
[ ] cargo tauri build --debug jalan (app kebuka, dashboard load)
[ ] Chat test via Playground (gateway key terisi)
[ ] Restart app → proxy auto-start, data persist di data/omniroute.db
[ ] (Opsional) Signing: Apple Developer ID + notarization
[ ] (Opsional) .dmg distribusi
```

---

## 9. Roadmap Lanjutan (setelah M0)

| M | Fitur | Lokasi kerja |
|---|---|---|
| M3 | Systray + daemon (events channel sudah disiapkan) | macOS |
| M4 | Auto-updater + bundling | macOS |
| Phase 4 | Cleanup JS OmniRoute (:20128) | Server |

---

*File ini bagian dari repo — update seiring perkembangan. Detail teknis
shell ada di `src-tauri/README.md`.*
