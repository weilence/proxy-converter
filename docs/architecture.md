# Architecture

Rust web service that serves token-bound mihomo/Clash YAML configs as `config.yaml` downloads, converts rule-providers to hosted `.mrs` files, and re-hosts geo databases per token. The admin UI is a Vue 3 + Nuxt UI SPA embedded into the binary at compile time. Build and deploy are container-only.

## Module map

- `src/main.rs` — clap CLI (`run` subcommand), `.env` loading, tracing init
- `src/server.rs` — axum router; public routes `GET /config?token=` and `GET /files/{name}?token=`; graceful shutdown
- `src/admin.rs` — `/admin` UI + `/admin/api/*`: cookie session auth (8h TTL), global failed-login lockout (in-memory, exponential backoff), token CRUD, orchestrates mrs/geo downloads and conversion
- `src/db.rs` — SeaORM/SQLite layer; schema is `CREATE TABLE IF NOT EXISTS` statements here (no migration framework)
- `src/entity/` — SeaORM entities (`token`, `hosted_file`)
- `src/mrs.rs` — pure-Rust encoder for mihomo's `.mrs` binary format (zstd + LOUDS trie); byte-compatible with `mihomo convert-ruleset`; no I/O
- `src/geo.rs` — downloads `geox-url` databases (geoip/geosite/mmdb/asn) and re-serves them as-is
- `src/assets.rs` — rust-embed of `frontend/dist/`, compiled only in release builds
- `frontend/` — admin SPA (Vue 3 + TS + Nuxt UI, Vite); no router, light theme only, served under `/admin/`
- `tests/fixtures/` — binary fixtures used by unit tests in `src/mrs.rs` (no integration test harness; tests are inline `mod tests`)

## Layering rules

- All state flows through `AppState` (Arc) shared by `server.rs` and `admin.rs`.
- Keep all SeaORM/SQLite access inside `src/db.rs`; handlers never touch the ORM directly.
- `mrs.rs` is pure computation (no I/O, no HTTP); `admin.rs` performs all downloading/fetching.
- Hosted files are private per token: names never collide across tokens (`db.rs` keys them by token id).
- `/files` authenticates only with the token's `file_key` (`?key=`), kept separate from the subscription `token` so a leaked config does not expose the config-fetch credential. `duplicate`/`reset-file-key` re-point every credential occurrence in the stored config at the new key.

## Gotchas

- Release builds embed `frontend/dist/` via rust-embed, so the frontend must be built first; the Dockerfile and CI encode this order. Debug builds compile `src/assets.rs` out entirely, so backend development needs no Node.
- `/admin` is entirely disabled (404) unless `ADMIN_PASSWORD` is set to a non-empty value. `.env` is loaded from cwd upward; real environment variables win.
- The dev SQLite db (`proxy-converter.db` + `-shm`/`-wal`) sits at the repo root and is gitignored; `--database` defaults to it. In containers the db must live on a mounted volume (`/data/tokens.db`).
- Inside a container the app must listen on `0.0.0.0` (the 127.0.0.1 default is unreachable through port mapping); the Dockerfile CMD fixes this.
- CI (`.github/workflows/docker-image.yml`) publishes only `ghcr.io/weilence/proxy-converter:latest`, built natively on amd64 + arm64 runners; see [deployment.md](deployment.md) before changing build or release flow.

## Conventions

- Conventional commits (`feat:`, `fix:`, `chore:`, `refactor:`).
- Code comments and identifiers in English; user-facing docs in Chinese.
- Logging via `tracing` with EnvFilter; default filter `info,sqlx=warn`, tunable via `RUST_LOG`.
