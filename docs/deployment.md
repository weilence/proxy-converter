# 容器构建与部署

构建与部署全部走容器：本地/CI 用多阶段 Dockerfile 构建镜像，CI 自动发布到 ghcr.io，服务器用 compose 运行。

## 镜像结构

[Dockerfile](../Dockerfile) 分三个阶段：Node 构建前端产物 → Rust 编译 release 二进制（前端产物经 rust-embed 内嵌）→ alpine 运行镜像（约 10 MB，含 ca-certificates 与 tzdata）。

要点：

- 容器内监听 `0.0.0.0:8080`（默认的 127.0.0.1 在端口映射下不可达）
- 数据库固定在 `/data/tokens.db`，必须挂载卷持久化
- `ADMIN_PASSWORD` 通过环境变量注入，不要构建进镜像

## 本地构建

```sh
docker build -t ghcr.io/weilence/proxy-converter:latest .
```

## CI 自动发布

每次 `main` 分支推送触发 [docker-image 工作流](../.github/workflows/docker-image.yml)：amd64 与 arm64 在原生 runner 上并行构建（公开仓库 arm64 runner 免费），按 digest 推送后合并为 `ghcr.io/weilence/proxy-converter:latest`——只有这一个 tag，不带版本号。

镜像默认私有：服务器 `docker login ghcr.io`（用户名 `weilence`，密码用带 `read:packages` 权限的 PAT）；也可在 package 设置里改为 public，则无需登录。

## 服务器部署

使用 [compose.yml](../compose.yml)：

```sh
docker login ghcr.io
docker compose up -d                          # 启动（SQLite 落在 ./data/）
docker compose pull && docker compose up -d   # 升级到最新 latest
docker logs -f proxy-converter                # 查看日志
```

## 旧镜像清理

[ghcr-cleanup 工作流](../.github/workflows/ghcr-cleanup.yml) 每天定时删除 ghcr 上 untagged 的旧版本（推新 `latest` 后旧版本即变 untagged），仓库只保留当前 `latest`。

前置配置：仓库 Settings → Secrets and variables → Actions 添加 `PACKAGE_CLEANUP_PAT`——classic PAT，勾选 `read:packages` 与 `delete:packages`。

注意事项：

- GitHub 定时任务使用 UTC，当前配置为每天北京时间 02:30
- 仓库连续 60 天无活动时定时任务会被自动停用，推送任意提交即可恢复
- 只保留 `latest` 意味着注册表侧无法回滚，回滚依赖服务器本地缓存的旧镜像；如需回滚能力，可把清理策略放宽为保留最近 3 个版本（`keep-n-most-recent`）
