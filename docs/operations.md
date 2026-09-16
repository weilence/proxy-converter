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
有效天数、绑定配置内容、复制令牌、重置文件密钥，以及 mrs / geo 托管文件管理，操作立即
生效无需重启。登录会话
为服务端内存 session（重启进程即全部失效），Cookie 仅携带不透明的 session ID，属性为
HttpOnly + SameSite=Strict；当反向代理传入 `X-Forwarded-Proto: https` 时自动附加
`Secure`，纯 HTTP 直连与本地开发不受影响。

为防密码爆破，连续 5 次密码错误后登录接口将全局锁定，锁定期间所有登录请求直接返回
429（含 `Retry-After` 头）；锁定时长从 1 分钟起逐次翻倍、上限 15 分钟，成功登录或重启
进程后重置。每次失败尝试会以 `warn` 级别记录来源 IP（优先取反向代理传入的
`X-Forwarded-For`，否则为连接地址）。

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
GET /files/{name}?key=<文件密钥>
```

下载管理后台为该令牌托管的文件：mrs 转换产物如 `/files/google.mrs`，geo 托管的
`/files/geoip`、`/files/geosite`、`/files/mmdb`、`/files/asn`。文件按令牌隔离，不同令牌
的文件名不会冲突。

文件密钥与订阅令牌相互独立：配置文件里内嵌的下载链接只携带文件密钥，即使配置外泄，
订阅令牌（可拉取配置本身）也不会随之泄露。文件密钥可在管理后台一键重置，重置后旧链接
立即失效，配置中的链接同时改写，客户端下次拉取配置时自动愈合。

## 日志

默认输出 `info` 级别，`RUST_LOG` 调整（如 `RUST_LOG=debug`）；容器下用
`docker logs -f proxy-converter`。
