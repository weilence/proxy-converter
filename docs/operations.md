# 运行与接口

## 启动

```sh
proxy-converter run [OPTIONS]
```

| 参数 | 简写 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `--addr <ADDR>` | `-a` | `127.0.0.1:8080` | HTTP 服务监听地址（容器内固定为 `0.0.0.0:8080`） |
| `--database <FILE>` | 无 | `proxy-converter.db` | SQLite 令牌库路径（容器内固定为 `/data/tokens.db`） |

## 环境变量

| 变量 | 说明 |
| ---- | ---- |
| `ADMIN_PASSWORD` | 管理后台密码；未设置或为空则 `/admin` 全部返回 404 |
| `RUST_LOG` | 日志过滤（EnvFilter 语法），默认 `info,sqlx=warn` |

环境变量可写入 `.env` 文件（从工作目录向上查找，真实环境变量优先；模板见
[.env.example](../.env.example)，文件已被 gitignore）。

## 管理后台

设置 `ADMIN_PASSWORD` 后，`/admin` 提供网页版管理：添加/删除令牌、启用/停用、设置备注、
有效天数、绑定配置内容，以及 mrs / geo 托管文件管理，操作立即生效无需重启。登录会话
有效期 8 小时，基于 HttpOnly + SameSite=Strict Cookie。

## 令牌

令牌有效需同时满足：已存在、处于启用状态且未过期；停用/启用立即生效。每次成功鉴权会
记录 `last_used_at`，便于清理长期未使用的令牌。

## 接口

```
GET /config?token=<令牌>
```

返回管理员为该令牌绑定的配置内容（`config.yaml` 附件）；令牌未绑定配置时返回 200 与空
响应体；无效令牌返回 401。

```
GET /files/{name}?token=<令牌>
```

下载管理后台为该令牌托管的文件：mrs 转换产物如 `/files/google.mrs`，geo 托管的
`/files/geoip`、`/files/geosite`、`/files/mmdb`、`/files/asn`。文件按令牌隔离，不同令牌
的文件名不会冲突。

## 日志

默认输出 `info` 级别，`RUST_LOG` 调整（如 `RUST_LOG=debug`）；容器下用
`docker logs -f proxy-converter`。
