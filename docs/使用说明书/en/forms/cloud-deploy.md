---
title: Cloud Deployment
nav_title: Deployment
summary: Cloud deployment: shipped with the Server — compose startup, ports and the post-deploy checklist.
updated_at: 2026-11-14
---

# Cloud Deployment

## Deployment

Cloud is not installed separately; it is deployed together with the Server, which Cloud and the Admin share. Deploy it once.

```bash
# x86
docker compose -f docker-compose-x86.yml up -d

# ARM
docker compose -f docker-compose-arm.yml up -d
```

- **Requirements**: Docker and Docker Compose (the recommended path), at least 4 GB of free memory, PostgreSQL (bundled in compose; SQLite works for a single machine); timezone defaults to `Asia/Shanghai`.
- **Access**: members open `http://<host>:18001/`; administrators use `http://<host>:18000/` for the Admin on the same host.
- **Ports**: Server API and Admin **18000** (`WUNDER_HOST` / `WUNDER_PORT`), Cloud **18001** (`NGINX_PORT`), sandbox 9001, onlyoffice 18080, drawio 18081, extra-mcp 18004, PostgreSQL 5432.
- The Cloud build output is served read-only by a separate web-service container and is **not hosted by the Server**.
- **Right after deploying**: change the administrator password → add LLM / TTS / image / video models under Model config → confirm the org tree → create preset agents → add users or open self-registration. Connect channels such as Feishu or WeCom only when you need them.
