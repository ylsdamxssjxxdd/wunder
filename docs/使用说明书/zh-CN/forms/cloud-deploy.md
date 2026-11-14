---
title: 云端版部署
nav_title: 部署
summary: 云端版部署：随服务端部署，compose 启动、端口分布与部署后清单。
updated_at: 2026-11-14
---

# 云端版部署

## 部署

云端版不单独安装，跟着服务端一起部署。服务端是云端版与管理端共用的运行前提，部署一次即可。

```bash
# x86 架构
docker compose -f docker-compose-x86.yml up -d

# ARM 架构
docker compose -f docker-compose-arm.yml up -d
```

- **前提**：Docker 与 Docker Compose（推荐方式）、至少 4 GB 可用内存、PostgreSQL（compose 已内置，单机也可改用 SQLite）；时区默认 `Asia/Shanghai`。
- **访问**：成员用浏览器打开 `http://<host>:18001/`；管理员用同一台机器的 `http://<host>:18000/` 进管理端。
- **端口**：服务端 API 与管理端 **18000**（`WUNDER_HOST` / `WUNDER_PORT` 可覆盖）、云端版 **18001**（`NGINX_PORT` 可覆盖）、沙箱 9001、onlyoffice 18080、drawio 18081、extra-mcp 18004、PostgreSQL 5432。
- 云端版的构建产物由独立的网页服务容器只读下发，**不由服务端托管**。
- **部署后先做这几件事**：改管理员口令 → 管理端「模型配置」加 LLM / TTS / 图像 / 视频模型 → 确认「单位管理」组织树 → 「预设智能体」建模板 → 「用户管理」加人或开放自助注册。需要飞书、企业微信、QQ Bot 等渠道时再在渠道里接入。
