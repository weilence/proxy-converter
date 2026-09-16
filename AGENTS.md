# AGENTS.md

Entry point for AI coding agents. Project in one line: a Rust (axum) web service that serves token-bound mihomo/Clash YAML configs as `config.yaml` downloads, converts rule-providers to hosted `.mrs` files, and re-hosts geo databases per token; a Vue 3 + Nuxt UI admin SPA is embedded in the binary. Build and deploy are container-only.

## Non-negotiable rules

- Release builds embed `frontend/dist/` via rust-embed — the frontend must be built first; the Dockerfile and CI encode this order. Debug builds compile `src/assets.rs` out entirely, so backend dev needs no Node.
- Keep all SeaORM/SQLite access in `src/db.rs`; the schema lives there as inline `CREATE TABLE IF NOT EXISTS` SQL (no migration framework).
- `/admin` is fully disabled (404) unless `ADMIN_PASSWORD` is non-empty.
- In containers the app listens on `0.0.0.0:8080` with the SQLite db on a mounted volume (`/data/tokens.db`).
- Conventional commits; code/comments in English, user-facing docs in Chinese.

## Commands

```sh
cargo run                          # debug server (API only, no UI) at 127.0.0.1:8080
cargo test                         # unit tests inline in src modules
cargo fmt && cargo clippy
cd frontend && npm run dev         # UI dev at localhost:5173/admin/ (proxies to :8080)
cd frontend && npm run typecheck   # vue-tsc
docker build -t ghcr.io/weilence/proxy-converter:latest .
```

## Read on demand

Deeper documentation is split by topic; read only what the current task touches:

- [docs/architecture.md](docs/architecture.md) — module map, layering rules, gotchas. Read before editing `src/`.
- [docs/deployment.md](docs/deployment.md) — image build, CI to ghcr (latest-only), cleanup cron. Read before touching `Dockerfile`, `.github/workflows/`, or `compose.yml`.
- [docs/development.md](docs/development.md) — dev environment details, frontend workflow, testing.
- [docs/operations.md](docs/operations.md) — runtime CLI, env vars, HTTP API, admin behavior. Read before changing request handling.
