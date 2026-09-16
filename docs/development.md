# 本地开发

## 后端（Rust）

```sh
cargo run          # debug 服务，监听 127.0.0.1:8080；debug 构建不含管理后台页面，仅提供 API
cargo test         # 单元测试（内联在各模块的 mod tests）
cargo fmt
cargo clippy
```

debug 构建不编译 `src/assets.rs`（rust-embed），因此后端开发完全不需要 Node。

## 前端（Vue 3 + Nuxt UI）

需要 Node.js ≥ 20，配合 Vite 开发服务器：

```sh
# 终端 1：后端 API（密码可写入 .env，见 [运行与接口](operations.md)）
cargo run

# 终端 2：前端开发服务器
cd frontend
npm install        # 首次；日常添加依赖也用它（可复现构建用 npm ci）
npm run dev        # http://localhost:5173/admin/，改动热更新
```

Vite 将 `/admin/api`、`/config`、`/files` 代理到 `127.0.0.1:8080`，前端改动即时生效，无需重编 Rust。

```sh
npm run typecheck  # vue-tsc
npm run build      # vite build + vue-tsc，产出 frontend/dist/
```

release 编译时 `frontend/dist/` 由 rust-embed 嵌入二进制，因此 release 构建（包括 Docker 构建）前必须先完成前端构建；Dockerfile 与 CI 已固化该顺序。

## 测试

单元测试内联在源码中（`src/db.rs`、`src/geo.rs`、`src/mrs.rs` 的 `mod tests`），其中 `src/mrs.rs` 使用 `tests/fixtures/` 下的二进制样例验证与 mihomo 的字节级兼容；没有独立的集成测试目录。
