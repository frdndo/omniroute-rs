# 🧪 M0 Desktop Build & Test Checklist (macOS)

Checklist untuk mem-build dan menguji aplikasi desktop **omniroute-rs** di
Mac, mulai dari prereq sampai release. Ikuti berurutan; centang tiap langkah.

> Bangun: `DESKTOP_BUILD.md` (instruksi build lengkap)

---

## A. Prepare (sekali per mesin)

- [ ] `xcode-select --install` (Xcode CLT)
- [ ] Rust via rustup: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` → `source ~/.cargo/env`
- [ ] Node LTS 24: `brew install node@24` (sama dengan CI; kalau keg-only, tambah ke PATH: `/opt/homebrew/opt/node@24/bin`)
- [ ] Tauri CLI: `cargo install tauri-cli --locked` (bukan cargo-tauri-cli!)
- [ ] `cd omniroute-rs && git pull` (ambil kode terbaru)

## B. Build Sidecar (binary proxy)

- [ ] `cd rust-core && CARGO_BUILD_JOBS=1 cargo build --release -p omniroute-core --bin server`
- [ ] Copy binary dengan suffix triple:
  - Apple Silicon: `cp target/release/server ../src-tauri/binaries/omniroute-server-aarch64-apple-darwin`
  - Intel: `cp target/release/server ../src-tauri/binaries/omniroute-server-x86_64-apple-darwin`
- [ ] Copy icon asli: `cd ../src-tauri && npm run tauri icon <logo-1024.png>`

## C. Build App

- [ ] `cd src-tauri && cargo tauri build --debug` ✅ wajib sebelum release
- [ ] Debug app kebuka tanpa crash
- [ ] `cargo tauri build` (release → .app + .dmg)

## D. Test Fungsional (app jalan)

### 1. Proxy auto-start
- [ ] Launch app → `curl -s http://127.0.0.1:20129/health` respon (200/401 = hidup)
- [ ] Log console app: "Proxy server starting on 0.0.0.0:20129"

### 2. Dashboard load
- [ ] Window kebuka, sidebar 15 menu tampil
- [ ] Login dengan admin key → Status page (health, provider count, model count)

### 3. Koneksi Provider (end-to-end routing)
- [ ] Providers → tambah koneksi openai (api_key test, priority 1)
- [ ] Playground → pilih model, kirim prompt → **response balik** (bukti routing ke upstream)
- [ ] Chat pake non-stream & stream (`stream: true`)
- [ ] Switch model kedua → fallback jalan kalau provider pertama fail

### 4. Fitur yang udah dibangun (M1-M10)
| Menu | Test | Harapan |
|---|---|---|
| Providers | CRUD | key di-mask `sk-a****`, toggle aktif, priority |
| API Keys | create | key penuh tampil SEKALI, bisa copy |
| Combos | create/del | chain model jalan di Playground |
| Logs | helm request | baris request muncul, filter status |
| Analytics | /admin/stats | total/status/by_provider/hourly |
| Costs | set pricing+budget | spend & used_pct terisi, badge budget |
| Webhooks | create | event chat.success terkirim (cek endpoint) |
| Cache | `{"cache":true}` 2x | request ke-2 HIT (hits +1) |
| MCP | /mcp tools/call chat | response via router |
| A2A | /a2a message/send | task completed + artifacts |
| Batch | submit 3 req | 3/3 succeeded, get status |
| Compress | `{"compress":true,"max_context_tokens":50}` | log "context compressed", respon tetap OK |
| Settings | /admin/settings | 16 fitur, registri 188/3012 |
| Docs | /admin/docs | 22 endpoint + env vars |

### 5. Persistensi
- [ ] Restart app → proxy auto-start lagi, DB/data/omniroute.db → provider & combo masih ada

### 6. Security
- [ ] Admin tanpa key → 503; key salah → 401
- [ ] Gateway tanpa key → 401
- [ ] Host spoof (Host header aneh) → 403
- [ ] Keys di dalam UI selalu masked (gak pernah tampil penuh di list)

## E. Smoke Test CLI (tanpa GUI, opsi cepat)

```bash
cd rust-core
OMNIROUTE_PORT=20129 OMNIROUTE_DB_PATH=./data/omniroute.db \
OMNIROUTE_ADMIN_KEYS=sk-admin OMNIROUTE_API_KEYS=sk-gateway \
OMNIROUTE_ALLOWED_HOSTS=localhost,127.0.0.1 \
CARGO_BUILD_JOBS=1 cargo run --release -p omniroute-core --bin server

# di terminal lain:
curl -s http://127.0.0.1:20129/health -H "Authorization: Bearer sk-gateway"
curl -s http://127.0.0.1:20129/v1/models | head
```

## F. Sebelum Release

- [ ] icon asli (bukan placeholder)
- [ ] `cargo tauri build` release .app kebuka, dashboard normal
- [ ] Restart app 2x → data persist, proxy auto-start
- [ ] (Opsional) Signing Apple Developer ID + notarization → hilangkan Gatekeeper prompt
- [ ] Distribusi .dmg

## G. Kalau Ada Yang Gagal

| Gejala | Cek |
|---|---|
| Sidecar "doesn't exist" | nama binary harus `omniroute-server-<triple>` (§B) |
| WebView blank | `VITE_PROXY_BASE` ter-set saat build dashboard (otomatis di beforeBuildCommand) |
| Gatekeeper "tak dikenal" | dev: `xattr -dr com.apple.quarantine omniroute-rs.app` |
| Port 20129 dipakai | set `OMNIROUTE_PORT` lain + rebuild dashboard dgn base URL sama |
| proxy hapus CORS | dashboard fetch lintas origin — pastikan CorsLayer::permissive aktif |

---

*Simpan sebagai `MAC_TEST_CHECKLIST.md`. Update sesuai kebutuhan setelah
build pertama di Mac.*