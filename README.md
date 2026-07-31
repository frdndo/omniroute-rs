# omniroute-rs

Rust port of the OmniRoute AI proxy/router — route any LLM through one
OpenAI-compatible endpoint. Multi-provider (175 providers, 2,888 models),
auto-fallback combos, account rotation, streaming, session affinity,
auto-combo scoring, and a hardened admin API — all in a single static binary.

## Why Rust?

- Single static binary (~10 MB), no Node.js runtime
- Memory-safe, ~5x lower memory than the TS/Next.js original
- Same feature core as OmniRoute v3.8.x: routing, combos, rotation,
  streaming, security hardening

## Quick start

```bash
# Build (server binary)
cargo build --release -p omniroute-core --bin server
# Target binary: rust-core/target/release/server

# Run
OMNIROUTE_PORT=20129 \
OMNIROUTE_PROVIDER_KEYS="openai=sk-xxx,sk-yyy;claude=sk-zzz" \
OMNIROUTE_ADMIN_KEYS="sk-admin" \
./rust-core/target/release/server
```

Point any OpenAI-compatible client at `http://<host>:20129/v1` with a gateway
API key (`OMNIROUTE_API_KEYS` or one created via the admin API).

## Configuration (env vars)

| Env var | Purpose | Example |
|---|---|---|
| `OMNIROUTE_PORT` | Listen port (default 20128) | `20129` |
| `OMNIROUTE_VERSION` | Version string override | `1.2.0` |
| `OMNIROUTE_PROVIDER_KEYS` | Upstream API keys per provider; comma = multiple accounts, semicolon = providers | `openai=sk-1,sk-2;claude=sk-3` |
| `OMNIROUTE_BASE_URL_<PROVIDER>` | Override upstream base URL (add `/v1` for OpenAI-style) | `http://127.0.0.1:9099/v1` |
| `OMNIROUTE_API_KEYS` | Gateway bearer keys (comma-separated); also loaded from DB `apiKeys` | `sk-gw-1,sk-gw-2` |
| `OMNIROUTE_ADMIN_KEYS` | Admin API keys. Unset → `/admin/*` returns 503 | `sk-admin` |
| `OMNIROUTE_ALLOWED_HOSTS` | Host header allowlist (comma). Empty = allow all (dev) | `localhost,127.0.0.1` |
| `OMNIROUTE_DB_PATH` | SQLite DB path (default `./data/omniroute.db`) | `/var/lib/omniroute.db` |
| `RUST_LOG` | Log level | `info` |

Config precedence (matches OmniRoute): **SQLite DB is the source of truth**
(provider connections, combos, API keys, session affinity, scorer stats);
env vars fill gaps at startup. Admin API changes apply live (hot reload).

## API

### OpenAI-compatible (`/v1/*`)

| Endpoint | Description |
|---|---|
| `POST /v1/chat/completions` | Chat, non-stream + SSE stream |
| `GET /v1/models` | All registry models |
| `GET /health` | Health check |

Extra headers: `X-Session-Id` enables session affinity (multi-turn stickiness).

### Admin (`/admin/*`, requires `OMNIROUTE_ADMIN_KEYS`)

| Endpoint | Description |
|---|---|
| `GET/POST /admin/providers` | List (keys masked) / create provider connections |
| `PUT/DELETE /admin/providers/{id}` | Update / delete |
| `GET/POST /admin/api-keys` | List (masked) / create gateway keys (full key returned once) |
| `PUT/DELETE /admin/api-keys/{id}` | Rename / enable-disable / delete |
| `GET/POST /admin/combos` | List / create fallback chains |
| `DELETE /admin/combos/{id}` | Delete combo |

## Feature parity with OmniRoute

| Feature | Status |
|---|---|
| Multi-provider routing | ✅ 175 providers / 2,888 models |
| Auto-combo fallback (DB-defined chains) | ✅ |
| Auto-combo scoring (health/latency/concurrency, persisted) | ✅ |
| Session affinity (`X-Session-Id`) | ✅ |
| Account rotation + cooldown (persisted) | ✅ |
| Streaming SSE (3 formats normalized) | ✅ |
| Gateway auth (env + DB keys) | ✅ |
| Host header guard | ✅ (403) |
| Admin CRUD (masked keys) | ✅ |
| MCP / A2A / i18n | ❌ (not in scope) |

## Architecture

```
rust-core/
├── omniroute-db/        # SQLite (rusqlite): migrations + repos
├── omniroute-providers/ # Provider registry catalog (JSON, generated)
└── omniroute-core/      # Axum proxy: routing, combo, account, auth, admin
```

Regenerate the provider catalog:

```bash
OMNIROUTE_SRC=/path/to/OmniRoute python3 scripts/extract_providers.py
```

## Development

```bash
cargo test                 # 112+ tests
cargo clippy --all-targets # must be 0 warnings
cargo fmt --all
```

CI (GitHub Actions): fmt → clippy -D warnings → test → build →
`rustsec/audit-check` (0 vulnerabilities).

## Security notes

- Admin API keys never returned after creation (masked: `sk-a****1234`)
- No admin keys configured → `/admin/*` fails closed (503)
- Gateway auth disabled only when no keys are configured at all (dev mode)
- Rate limiting per IP; host guard blocks spoofed `Host` headers
