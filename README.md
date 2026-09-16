# proxy-converter

管理令牌绑定的 YAML 订阅配置（mihomo/Clash），以 `config.yaml` 附件形式提供下载；支持把配置中的 rule-provider 转换为托管 `.mrs` 文件、代为托管 geo 数据库，并提供网页版管理后台。

Rust（axum）单二进制，管理后台（Vue 3 + Nuxt UI）在编译期内嵌；容器化构建，CI 自动发布 amd64/arm64 镜像到 ghcr.io。

## 快速开始

```sh
mkdir -p data
docker run -d --name proxy-converter -p 8080:8080 \
  -e ADMIN_PASSWORD=your-password -v "$PWD/data:/data" \
  ghcr.io/weilence/proxy-converter:latest
```

浏览器打开 `http://<主机>:8080/admin` 登录管理后台；客户端以 `GET /config?token=<令牌>` 拉取配置。私有镜像需先 `docker login ghcr.io`，见[部署文档](docs/deployment.md)。

## 文档

- [本地开发](docs/development.md) — 前后端开发环境与测试
- [构建与部署](docs/deployment.md) — Dockerfile、CI 发布 ghcr、compose 部署与镜像清理
- [运行与接口](docs/operations.md) — 启动参数、环境变量、管理后台与 HTTP 接口
- [架构说明](docs/architecture.md) — 模块划分、分层规则与代码约定（面向 AI 助手与新贡献者）

## 许可

[MIT](LICENSE)
