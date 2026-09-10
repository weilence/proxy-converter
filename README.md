# proxy-converter

拉取远程 YAML 订阅配置，可选地通过 JavaScript 脚本转换后，以 `config.yaml` 附件形式提供下载。

## 使用

```sh
proxy-converter run [OPTIONS]        # 启动服务
proxy-converter token <ACTION>      # 管理令牌
```

`run` 子命令参数：

| 参数 | 简写 | 默认值 | 说明 |
| ---- | ---- | ------ | ---- |
| `--addr <ADDR>` | `-a` | `127.0.0.1:8080` | HTTP 服务监听地址 |
| `--script <FILE>` | `-s` | 无 | 定义 `main(data)` 的 JS 转换脚本 |
| `--database <FILE>` | 无 | `proxy-converter.db` | SQLite 令牌库路径（全局参数，也可用于 `token` 子命令） |

示例：

```sh
proxy-converter run --addr 0.0.0.0:8080 --script transform.js --database tokens.db
```

## 令牌管理

访问令牌存储在 SQLite 数据库中，通过子命令管理：

```sh
proxy-converter token add <TOKEN> [-n <名称>] [-d <有效天数>]   # 新增令牌（可命名、可设有效期）
proxy-converter token list                                     # 查看全部令牌及状态
proxy-converter token remove <TOKEN>                           # 删除令牌
proxy-converter token enable <TOKEN>                           # 启用令牌
proxy-converter token disable <TOKEN>                          # 停用令牌
```

令牌有效需同时满足：已存在、处于启用状态且未过期。停用/启用立即生效，无需重启服务；
每次成功鉴权会记录 `last_used_at`，便于清理长期未使用的令牌。

## 管理后台

设置环境变量 `ADMIN_PASSWORD` 后，服务在 `/admin` 提供网页版令牌管理后台（页面源码位于 `frontend/` 目录，编译时内嵌进二进制）：

```sh
ADMIN_PASSWORD=your-password proxy-converter run
```

- 浏览器打开 `http://127.0.0.1:8080/admin`，输入管理员密码登录
- 支持添加/删除令牌、启用/停用、设置备注与有效天数，操作立即生效
- 未设置 `ADMIN_PASSWORD`（或留空）时后台自动禁用，所有 `/admin` 路由返回 404
- 登录会话有效期 8 小时，基于 HttpOnly + SameSite=Strict Cookie

## 接口

```
GET /convert?url=<远程YAML地址>&token=<令牌>
```

令牌通过 `token` 查询参数携带，必须是数据库中有效的令牌，否则返回 401。
服务端拉取 `url` 指向的 YAML，解析后（若提供了 `--script`）交给脚本中的
`main(data)` 处理，最后重新序列化为 YAML 返回。

## 转换脚本

脚本需要定义一个 `main` 函数，接收配置对象并返回转换后的对象：

```js
function main(data) {
  data["mixed-port"] = 7897;
  data.proxies = (data.proxies || []).filter((p) => p.type !== "direct");
  return data;
}
```

## 日志

默认输出 `info` 级别日志，可通过 `RUST_LOG` 环境变量调整，例如 `RUST_LOG=debug`。
