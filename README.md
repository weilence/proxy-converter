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

设置环境变量 `ADMIN_PASSWORD` 后，服务在 `/admin` 提供网页版令牌管理后台（页面源码位于 `frontend/` 目录，编译时内嵌进二进制）：

```sh
ADMIN_PASSWORD=your-password proxy-converter run
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

> `GET /convert` 已废弃：现在返回 302 重定向到 `/config`（自动携带原有查询参数），
> 请尽早迁移到新接口。

## 部署（systemd）

仓库根目录提供 [proxy-converter.service](proxy-converter.service) 模板：

```sh
cargo build --release
sudo install -m 755 target/release/proxy-converter /usr/local/bin/

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

## 日志

默认输出 `info` 级别日志，可通过 `RUST_LOG` 环境变量调整，例如 `RUST_LOG=debug`。
