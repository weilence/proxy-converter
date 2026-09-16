# proxy-converter

管理令牌绑定的 YAML 订阅配置，以 `config.yaml` 附件形式提供下载。

## 使用

```sh
proxy-converter run [OPTIONS]        # 启动服务
```

`run` 子命令参数：

| 参数 | 简写 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `--addr <ADDR>` | `-a` | `127.0.0.1:8080` | HTTP 服务监听地址 |
| `--database <FILE>` | 无 | `proxy-converter.db` | SQLite 令牌库路径 |

示例：

```sh
proxy-converter run --addr 0.0.0.0:8080 --database tokens.db
```

## 令牌管理

访问令牌存储在 SQLite 数据库中，统一通过管理后台（见下文）管理：添加/删除令牌、启用/停用、
设置备注、有效天数与绑定的配置内容。令牌有效需同时满足：已存在、处于启用状态且未过期，
停用/启用立即生效，无需重启服务；每次成功鉴权会记录 `last_used_at`，便于清理长期未使用的令牌。

## 管理后台

设置环境变量 `ADMIN_PASSWORD` 后，服务在 `/admin` 提供网页版令牌管理后台（页面源码位于 `frontend/` 目录，Vue 3 + Nuxt UI 构建，release 编译时内嵌进二进制；debug 构建不提供页面、仅提供 API，见下文[前端开发](#前端开发)）：

```sh
ADMIN_PASSWORD=your-password proxy-converter run
```

环境变量也可以写进 `.env` 文件（从工作目录向上查找，真实环境变量优先；模板见
[.env.example](.env.example)，文件已被 gitignore）：

```sh
cp .env.example .env   # 然后填入 ADMIN_PASSWORD
```

- 浏览器打开 `http://127.0.0.1:8080/admin`，输入管理员密码登录
- 支持添加/删除令牌、启用/停用、设置备注、有效天数与配置内容，操作立即生效
- 未设置 `ADMIN_PASSWORD`（或留空）时后台自动禁用，所有 `/admin` 路由返回 404
- 登录会话有效期 8 小时，基于 HttpOnly + SameSite=Strict Cookie

## 接口

```
GET /config?token=<令牌>
```

令牌通过 `token` 查询参数携带，必须是数据库中有效的令牌，否则返回 401。
返回管理员为该令牌绑定的配置内容；令牌未绑定配置内容时返回 200 与空内容（空响应体）。

```
GET /files/{name}?token=<令牌>
```

下载管理后台为该令牌托管的文件，如 mrs 转换产出的 `/files/google.mrs`、geo 托管的
`/files/geoip`、`/files/geosite`、`/files/mmdb`、`/files/asn`。`GET /mrs/{name}` 为
改版前的旧路径，仍然兼容。

> `GET /convert` 已废弃：现在返回 302 重定向到 `/config`（自动携带原有查询参数），
> 请尽早迁移到新接口。

## 前端开发

管理后台前端位于 `frontend/`（Vue 3 + TypeScript + Nuxt UI，Vite 构建）。后端 debug
开发不需要 Node；前端开发需要 Node.js ≥ 20，配合 Vite dev server 进行：

```sh
# 终端 1：后端 API（debug 构建仅提供接口；密码可写入 .env）
cargo run

# 终端 2：Vite 开发服务器（/admin/api、/config 代理到 127.0.0.1:8080）
cd frontend
npm install
npm run dev
```

浏览器打开 <http://localhost:5173/admin/>，前端改动热更新，无需重编 Rust。

`npm run build` 产出 `frontend/dist/`（同跑 `vue-tsc` 类型检查）；release 编译时由
[rust-embed](https://crates.io/crates/rust-embed) 将其嵌入二进制，因此 **release 编译前必须先
完成前端构建**（[scripts/release.sh](scripts/release.sh) 已按此顺序执行）。

## 部署（systemd）

仓库根目录提供 [proxy-converter.service](proxy-converter.service) 模板：

```sh
./scripts/release.sh
sudo install -m 755 target/release/proxy-converter /usr/local/bin/
```

脚本等价于先 `cd frontend && npm ci && npm run build`（前端产物内嵌进二进制），再
`cargo build --release`。

# 创建无登录权限的专用运行用户
sudo useradd --system --user-group --home-dir /nonexistent --shell /usr/sbin/nologin proxy-converter

# 管理后台密码（不设置则后台禁用，仅 /config 可用）
sudo mkdir -p /etc/proxy-converter
echo 'ADMIN_PASSWORD=your-password' | sudo tee /etc/proxy-converter/proxy-converter.env
sudo chmod 600 /etc/proxy-converter/proxy-converter.env

sudo install -m 644 proxy-converter.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now proxy-converter
journalctl -u proxy-converter -f      # 查看日志
```

数据库默认落在 `/var/lib/proxy-converter/tokens.db`（由 `StateDirectory` 自动创建并授权）；
监听地址、数据库路径等按需修改 unit 文件中的 `ExecStart` 后执行
`sudo systemctl restart proxy-converter`。

### 在 macOS 上交叉编译 linux/amd64

一次性准备：

```sh
brew install messense/macos-cross-toolchains/x86_64-unknown-linux-musl
rustup target add x86_64-unknown-linux-musl
```

并把下面的配置写入 `~/.cargo/config.toml`（机器级配置，不随仓库走；若文件已有内容则追加）：

```toml
[target.x86_64-unknown-linux-musl]
linker = "x86_64-unknown-linux-musl-gcc"

[env]
CC_x86_64_unknown_linux_musl = "x86_64-unknown-linux-musl-gcc"
AR_x86_64_unknown_linux_musl = "x86_64-unknown-linux-musl-ar"
```

构建（release 二进制内嵌前端产物，需先构建前端）：

```sh
(cd frontend && npm ci && npm run build)
cargo build --release --target x86_64-unknown-linux-musl
```

产物 `target/x86_64-unknown-linux-musl/release/proxy-converter` 为全静态二进制，
不依赖目标机任何库，scp 到任意 x86_64 Linux 即可运行。

> 仓库本身不携带任何交叉编译配置：在 Linux 上直接 `cargo build --release` 原生构建即可；
> 如确需在 Linux 上编 musl 目标，安装 `musl-tools` 后以环境变量指定
> `CC_x86_64_unknown_linux_musl=musl-gcc` 和 `CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc`。

## 日志

默认输出 `info` 级别日志，可通过 `RUST_LOG` 环境变量调整，例如 `RUST_LOG=debug`。
