# wunder-desktop 在线更新部署指南（Electron + Tauri）

## 1. 目标

- 让桌面端用户可在“系统设置 -> 检测更新”里发现并安装新版本。
- 同时覆盖两条桌面打包链路：
  - `wunder-desktop-electron`（Electron + electron-updater）
  - `wunder-desktop`（Tauri + tauri-plugin-updater）

---

## 2. 两条更新链路的差异

- Electron 使用 `latest.yml + installer.exe + installer.exe.blockmap`。
- Tauri 使用签名校验，更新元数据是 JSON（动态或静态格式），并要求有效公钥。
- 两条链路都可以用自建 Nginx 静态服务托管。

---

## 3. Electron（wunder-desktop-electron）

### 3.1 推荐配置（Generic）

编辑 `wunder-desktop-electron/electron-builder.yml`：

```yaml
publish:
  provider: generic
  url: https://updates.example.com/wunder-desktop/win/x64
```

建议固定文件名（避免空格和重命名问题）：

```yaml
win:
  artifactName: "Wunder-Desktop-Setup-${version}.${ext}"
```

### 3.2 发布文件

构建：

```bash
cd wunder-desktop-electron
npm run build
```

`dist/` 需上传：

- `latest.yml`
- `Wunder-Desktop-Setup-<version>.exe`
- `Wunder-Desktop-Setup-<version>.exe.blockmap`

> `latest.yml` 的 `path/sha512/size` 必须与实际文件一致。

### 3.3 Nginx 关键点

- `latest.yml` 必须禁缓存：

```nginx
location = /wunder-desktop/win/x64/latest.yml {
  add_header Cache-Control "no-cache, no-store, must-revalidate";
  add_header Pragma "no-cache";
  add_header Expires "0";
}
```

---

## 4. Tauri（wunder-desktop）

## 4.1 当前仓库接入方式

- 已接入 `tauri-plugin-updater`。
- 前端通过 `window.wunderDesktop` 调用：
  - `checkForUpdates`
  - `getUpdateState`
  - `installUpdate`
- Tauri 打包已开启更新产物生成：`wunder-desktop/tauri.conf.json` 中 `bundle.createUpdaterArtifacts = true`。

### 4.2 运行时配置（环境变量）

当前实现采用环境变量注入更新源：

- `WUNDER_TAURI_UPDATE_ENDPOINTS`：逗号分隔 URL 列表（支持 `{{target}}/{{arch}}/{{current_version}}` 占位符）。
- `WUNDER_TAURI_UPDATE_PUBKEY`：Tauri updater 的 minisign 公钥。

示例：

```bash
set WUNDER_TAURI_UPDATE_ENDPOINTS=https://updates.example.com/wunder-desktop/tauri/latest.json
set WUNDER_TAURI_UPDATE_PUBKEY=RWQ...你的公钥...
```

若未配置，桌面端会返回：`update source is not configured`。

### 4.3 产物与服务端

使用 Tauri 打包后会生成安装包及 updater 产物（含签名文件），将这些文件上传到你的更新服务目录；更新接口返回 JSON 元数据并指向对应安装包 URL。

---

## 5. 迁移注意事项（GitHub -> 自建）

- 已安装老版本客户端的更新源写在其内置配置中。
- 不能指望老客户端自动“换源”。
- 建议：先发一个“过渡安装包”（已切到新源），让用户手动安装一次，后续再走自动更新。

---

## 6. 发布 SOP

1. 提升版本号。
2. 打包并核对 updater 元数据与文件名。
3. 上传新安装包与元数据（覆盖最新元数据文件）。
4. 校验元数据 HTTP 缓存头（必须避免强缓存）。
5. 用一台旧版本客户端执行“检测更新”做冒烟验证。

---

## 7. 常见问题

### Q1：检测更新提示 `update source is not configured`

- Electron：`app-update.yml` 或 `publish` 配置缺失。
- Tauri：未设置 `WUNDER_TAURI_UPDATE_ENDPOINTS / WUNDER_TAURI_UPDATE_PUBKEY`。

### Q2：服务端已更新但客户端仍显示最新版

优先排查：

- 元数据被缓存。
- 元数据中的版本号未提升。
- 元数据里的下载 URL 与线上文件不一致。
