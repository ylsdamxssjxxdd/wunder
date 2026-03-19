# wunder 补全文档落地方案

## 1. 背景与目标

截至 2026-03-19，wunder 已经具备较完整的后端、桌面端、用户侧前端、管理端、渠道、工具、蜂群、长期记忆与评估能力，但文档形态仍以“仓库内散落 Markdown + 少量展示页”为主，尚未形成像 OpenClaw 一样可导航、可搜索、可网页访问的文档站。

本方案的目标不是简单“补几篇文档”，而是同时解决三件事：

1. 建立 wunder 的正式文档源目录，避免 `docs/` 根目录继续无序膨胀。
2. 将现有系统/设计/API/方案文档重组为面向用户、开发者、管理员都能使用的文档体系。
3. 在不破坏现有 Rust + Axum + 原生 `web/` 架构的前提下，实现类似 OpenClaw 的网页式访问体验。

当前约定同步更新如下：

- 方案文档保留在 `docs/wunder补全文档落地方案.md`
- 后续正式静态站文档统一落在 `docs/静态站文档/`

## 2. 参考分析

### 2.1 OpenClaw 线上文档的可借鉴点

参考站点：

- OpenClaw 文档站：<https://docs.openclaw.ai/zh-CN>
- OpenClaw 仓库：<https://github.com/openclaw/openclaw>

对照 `C:\Users\32138\Desktop\参考项目\openclaw-main` 可见，OpenClaw 的文档工程有几个关键特征：

1. 文档源目录集中在 `docs/`，不是散落在多个子系统中。
2. 通过 `docs/docs.json` 统一维护站点导航、语言、分组、重定向与首页结构。
3. 中文文档使用 `docs/zh-CN/**` 独立维护，多语言前缀清晰。
4. 首页不是“README 直出”，而是卡片式入口页，强调“快速开始 / 核心概念 / Web 界面 / 渠道 / 工具 / 参考”。
5. 文档工程具备独立开发、校验与链接检查脚本，例如 `docs:dev`、`check:docs`、`docs:check-links`。
6. 文档目录中同时存在 “hubs / docs-directory / faq / troubleshooting / reference” 等导航中枢页，而不是只靠左侧树状目录。

### 2.2 OpenClaw 的实现方式与 wunder 的差异

OpenClaw 当前采用 Mintlify 文档工程，适合公开站点、SEO、多语言与站内搜索；但 wunder 当前代码结构与 OpenClaw 不同：

- wunder 后端已经通过 `src/main.rs` 将 `web/` 作为静态站点直接挂载。
- wunder 已有原生文档渲染能力：`web/modules/paper.js` 能渲染 Markdown，`web/modules/api-docs.js` 能加载 JSON 清单式文档。
- wunder 的管理端与用户端都不是 Next.js/Mintlify 栈，直接引入一套新的文档框架会增加额外构建链路、部署复杂度与维护成本。
- wunder 当前 `docs/` 目录里混合了“正式文档 / 内部方案 / 演示资料 / PPT / 视频提示词”，如果直接套 Mintlify，只会把混乱公开化。

结论：

- 内容组织上，应该学习 OpenClaw。
- 技术实现上，不建议第一阶段直接照搬 Mintlify。
- wunder 更适合先做“自托管静态文档站”，等文档内容稳定后，再决定是否导出到独立公开站点。
- 文档页面的布局、内容组织、说法方式应尽量贴近 OpenClaw 当前的写法，而不是延续 wunder 现有超长方案文档风格。

## 3. wunder 现状问题

### 3.1 文档内容层问题

当前 `docs/` 目录的主要问题：

1. 缺少正式文档源目录。正式文档、方案文档、演示资料、论文材料混放。
2. 缺少统一导航模型。用户不知道先读哪个，维护者也不知道新文档该落在哪。
3. 大量核心内容只存在于超长文档中，例如 `docs/系统介绍.md`、`docs/设计方案.md`、`docs/API文档.md`，不适合网页浏览。
4. 缺少文档中枢页、FAQ、排障页、快速开始页、角色化入口页。
5. 缺少明确的“哪些是正式文档，哪些是内部方案/RFC”的边界。

### 3.2 网页访问层问题

当前代码基础说明：

- `src/main.rs` 只直接挂载了 `web/`、`docs/ppt/`、`docs/ppt-en/`。
- `docs/` 仓库目录本身并不会自动公开为网页。
- `web/index.html` 中的文档能力目前只包含：
  - 幻灯片
  - 单篇论文预览
  - API 文档 JSON 预览
- `src/core/auth.rs` 对 `/wunder/*` 默认按管理路径处理，如果未来把文档站挂到 `/wunder/docs/*`，还需要显式放行。

结论：

- 仅把 Markdown 放进 `docs/静态站文档/` 还不够。
- 必须同时规划“文档源目录 -> 发布目录 -> 浏览器访问路径”的映射。

## 4. 总体方案

### 4.1 方案原则

1. `docs/静态站文档/` 作为正式文档源目录。
2. `docs/方案/` 继续保留为内部分析/RFC，不直接作为正式文档站主导航。
3. 文档站发布目录放在 `web/docs/`，通过现有静态资源服务直接公开。
4. 访问路径使用 `/docs/`，避免走 `/wunder/*` 触发鉴权分支。
5. 第一阶段复用原生 HTML + JS + Markdown 渲染能力，先把内容体系跑通。
6. 第二阶段补全文档站交互能力：目录、搜索、上一篇/下一篇、同页锚点、多语言。
7. 后续若需要独立公开域名，可再导出为 Mintlify/Fumadocs 之类的外部站点。
8. 文档单页结构、frontmatter、入口页、Hub 页、FAQ 页尽量参考 OpenClaw 的组织方式。

### 4.2 推荐的目录分层

建议将仓库内文档分成三层：

#### A. 正式文档源

路径：

```text
docs/静态站文档/
```

用途：

- 面向用户、管理员、开发者的正式说明
- 可被网页文档站直接消费
- 必须遵守统一 frontmatter 与导航约定

#### B. 内部方案/RFC

路径：

```text
docs/方案/
```

用途：

- 设计草案、对标分析、实验记录、迁移方案
- 不直接进入公开文档主导航
- 只在正式文档中按需引用

#### C. 展示与素材

路径：

```text
docs/ppt/
docs/ppt-en/
docs/diagrams/
docs/视频提示词/
```

用途：

- 演示、图示、素材输出
- 不作为正式文档的主内容源

## 5. 正式文档信息架构

### 5.1 顶层分类

参考 OpenClaw 的“Get Started / Concepts / Tools / Web / Reference / Help”结构，建议 wunder 第一版采用以下顶层分类：

1. 首页
2. 快速开始
3. 运行形态
4. 核心概念
5. 工具与技能
6. 接口接入
7. 前端与桌面
8. 运维治理
9. 参考
10. 帮助与排障

### 5.2 推荐目录树

建议正式文档源目录采用：

```text
docs/静态站文档/
  zh-CN/
    index.md
    start/
      quickstart.md
      desktop.md
      server.md
      cli.md
      docs-directory.md
      hubs.md
    modes/
      overview.md
      server.md
      cli.md
      desktop.md
      remote-gateway.md
    concepts/
      architecture.md
      five-dimensions.md
      sessions-and-rounds.md
      workspace.md
      tools.md
      memory.md
      swarm.md
      user-world.md
    tools/
      index.md
      builtin-tools.md
      mcp.md
      skills.md
      knowledge.md
      approvals.md
    integration/
      wunder-api.md
      chat-ws.md
      a2a.md
      mcp-endpoint.md
      channel-webhook.md
    surfaces/
      frontend.md
      web-admin.md
      desktop-ui.md
    ops/
      deployment.md
      auth-and-security.md
      monitoring.md
      benchmark.md
      performance.md
    reference/
      config.md
      api-index.md
      tool-catalog.md
      glossary.md
      error-codes.md
    help/
      faq.md
      troubleshooting.md
```

后续多语言扩展时再追加：

```text
docs/静态站文档/en-US/
```

### 5.3 首批必须补齐的页面

第一批不求全，但必须把入口页与主链路补齐。建议按以下优先级落地：

1. `index.md`
2. `start/quickstart.md`
3. `start/desktop.md`
4. `start/server.md`
5. `concepts/architecture.md`
6. `concepts/sessions-and-rounds.md`
7. `concepts/tools.md`
8. `concepts/memory.md`
9. `integration/wunder-api.md`
10. `integration/chat-ws.md`
11. `surfaces/web-admin.md`
12. `ops/deployment.md`
13. `ops/auth-and-security.md`
14. `reference/config.md`
15. `help/faq.md`
16. `help/troubleshooting.md`

## 6. 现有文档到新体系的映射

建议按“拆分、引用、归档”三种方式处理现有文档：

| 现有文档 | 处理方式 | 目标位置 |
| --- | --- | --- |
| `README.md` | 拆分 | `zh-CN/index.md`、`start/quickstart.md` |
| `docs/系统介绍.md` | 拆分 | `concepts/architecture.md`、`modes/overview.md`、`surfaces/*.md` |
| `docs/设计方案.md` | 拆分 | `concepts/*.md`、`ops/*.md`、`reference/config.md` |
| `docs/API文档.md` | 拆分 + 保留原件 | `integration/*.md`、`reference/api-index.md` |
| `docs/用户手册.md` | 合并重写 | `start/desktop.md`、`help/faq.md` |
| `docs/用户前端.md` | 合并重写 | `surfaces/frontend.md` |
| `docs/测试大纲.md` | 拆分 | `ops/benchmark.md`、`ops/performance.md` |
| `docs/方案/*.md` | 引用或归档 | 保持内部方案，不进入主导航 |

处理原则：

1. 原始大文档不立即删除，先保留作为事实来源。
2. 正式文档站页面必须更短、更聚焦、更适合网页阅读。
3. “系统原理”与“实施细节”要分开写，避免页面再次超长。

## 7. 网页式访问实现方案

### 7.1 推荐实现：复用现有静态站能力

推荐路径：

1. 文档源放在 `docs/静态站文档/zh-CN/**/*.md`
2. 通过脚本生成发布产物到 `web/docs/`
3. 由当前 `src/main.rs` 对 `web/` 的静态挂载直接对外提供 `/docs/`

这样做的优点：

- 不需要新增一套文档服务
- 不需要引入额外 Node 文档框架
- 能直接复用当前 Axum 静态服务
- 能和管理端部署方式保持一致
- 对 desktop/server 都友好

### 7.2 发布目录建议

建议新增如下结构：

```text
web/docs/
  index.html
  app.js
  app.css
  manifest.json
  search.json
  content/
    zh-CN/
      index.md
      start/
      concepts/
      ...
  assets/
    diagrams/
    images/
```

其中：

- `manifest.json`：导航树、页面标题、slug、顺序、上一页/下一页关系
- `search.json`：站内搜索索引
- `content/**/*.md`：发布后的 Markdown 内容
- `assets/`：文档图示与附图

### 7.3 路由建议

推荐 URL 设计：

```text
/docs/
/docs/zh-CN/
/docs/zh-CN/start/quickstart
/docs/zh-CN/concepts/architecture
/docs/zh-CN/reference/config
```

不推荐第一阶段使用：

```text
/wunder/docs/*
```

原因：

- 当前 `src/core/auth.rs` 会把 `/wunder/*` 默认视为受保护路径。
- 文档站应尽量独立于业务 API 鉴权逻辑。

### 7.4 页面能力清单

第一阶段文档站应具备以下能力：

1. 首页卡片导航
2. 左侧目录树
3. 正文 Markdown 渲染
4. 同页目录（On this page）
5. 上一篇 / 下一篇
6. 站内关键字搜索
7. 代码块复制
8. 图片与 SVG 正常显示
9. 中英文入口预留

第二阶段再补：

1. 全文搜索高亮
2. 重定向与旧链接兼容
3. FAQ 聚合页
4. 多语言切换
5. SEO 元信息

## 8. 技术落地设计

### 8.1 生成脚本

建议新增脚本：

```text
scripts/build_docs_site.py
```

脚本职责：

1. 扫描 `docs/静态站文档/zh-CN/**/*.md`
2. 读取 frontmatter
3. 生成 `web/docs/manifest.json`
4. 生成 `web/docs/search.json`
5. 复制 Markdown 到 `web/docs/content/`
6. 复制引用的图示资源到 `web/docs/assets/`
7. 做基础链接校验

### 8.2 frontmatter 规范

每篇正式文档建议统一使用更接近参考项目的 frontmatter，避免继续沿用方案文档的长标题/重说明风格：

```yaml
---
title: 快速开始
summary: 5 分钟了解 wunder 的三种运行形态与首条对话链路
read_when:
  - 你第一次部署 wunder
  - 你想先跑通 desktop、server、cli 中的一条主链路
source_docs:
  - README.md
  - docs/系统介绍.md
---
```

这样做的目的：

- 页面头部形式尽量贴近 OpenClaw 的页面风格
- 页面文件路径天然承担 slug 角色，减少 frontmatter 冗余
- 生成 manifest 时仍可从目录结构推导 slug、分组与排序
- 保留“该页面内容来自哪些原始文档”的追踪信息

补充约定：

- `slug/group/order` 尽量由生成脚本根据目录和导航配置推导，不强制写死在每个 Markdown 中。
- 页面开头应先给出一句话摘要，再进入正文，不建议用大段背景铺垫。
- 单篇文档优先解决一个问题，参考项目中的 `start/`、`concepts/`、`help/` 页面风格。

### 8.3 前端实现建议

建议新增：

```text
web/docs/index.html
web/docs/app.js
web/docs/app.css
```

并复用现有能力：

- Markdown 渲染可参考 `web/modules/paper.js`
- JSON 驱动的详情页/导航模式可参考 `web/modules/api-docs.js`
- 样式保持轻量，不使用 `backdrop-filter`

页面结构建议：

1. 顶栏：品牌、语言切换、搜索框、GitHub/主页入口
2. 左栏：分类导航
3. 中栏：Markdown 正文
4. 右栏：本页目录 + 相关推荐

### 8.4 对现有代码的影响

第一阶段尽量不改后端逻辑，只做静态资源扩展。

必要调整只有两类：

1. 如需将管理端旧文档资源与新文档站分离，可把现有 `web/docs/api-docs.json`、`web/docs/paper.md` 迁移到 `web/docs/admin/`。
2. 如未来坚持把文档挂到 `/wunder/docs/`，则必须同步调整 `src/core/auth.rs` 的放行规则。

## 9. 内容编写规范

### 9.1 页面写法

每篇页面建议统一结构，尽量参考 OpenClaw 的页面节奏：

1. 这篇文档解决什么问题
2. 适用对象
3. 前置条件
4. 步骤/原理
5. 验证方式
6. 常见问题
7. 相关文档

### 9.2 内容边界

正式文档站只收三类内容：

1. 面向用户的使用说明
2. 面向开发者/管理员的稳定接口与稳定机制
3. 面向运维的部署、治理、排障与参考

以下内容不直接进主导航：

1. 临时方案
2. 纯对标分析
3. 演讲脚本
4. 视频提示词
5. 一次性实验记录

### 9.3 文风要求

建议采用 OpenClaw 的长处，但保留 wunder 自身语义：

- 首页要短，强调入口
- 快速开始要直接，不绕弯
- 原理页要结构化，不堆口号
- 接口页要可复制、可验证
- 排障页要按问题组织，不按模块自说自话
- 多用 “What/Why/How/Related docs” 这种清晰切分，而不是一页里混合目标、方案、实现、结论
- 尽量一页一个主题，减少 wunder 现有“大而全单文档”写法

## 10. 实施排期

### 阶段 1：整理内容源

目标：

- 建立 `docs/静态站文档/zh-CN/`
- 输出首批 16 篇核心页面
- 明确正式文档与内部方案边界

产出：

- 目录树
- frontmatter 规范
- 首批页面文案

### 阶段 2：网页文档站

目标：

- 打通 `docs/静态站文档 -> web/docs`
- 通过 `/docs/` 访问
- 完成目录、正文、同页目录、搜索

产出：

- `scripts/build_docs_site.py`
- `web/docs/index.html`
- `web/docs/app.js`
- `web/docs/app.css`
- `web/docs/manifest.json`

### 阶段 3：增强与外部发布

目标：

- 多语言
- 旧链接重定向
- 搜索优化
- 独立域名或独立站点

可选路线：

1. 继续使用 wunder 自托管静态文档站
2. 从同一内容源自动导出 Mintlify 站点

## 11. 验收标准

完成后至少满足以下标准：

1. `docs/静态站文档/zh-CN/` 成为正式文档唯一来源目录。
2. 用户能通过 `/docs/` 在浏览器访问 wunder 文档站。
3. 首页、快速开始、核心概念、接口接入、帮助与排障五大入口可直达。
4. 每篇正式文档都有唯一 slug、标题、摘要、导航归属。
5. 文档站支持同页目录、上一篇/下一篇、搜索。
6. 现有 `docs/系统介绍.md`、`docs/设计方案.md`、`docs/API文档.md` 中的核心内容已被拆分吸收，不再依赖单篇超长文档作为唯一入口。
7. 内部方案类文档继续保留，但不污染正式文档主导航。

## 12. 明确建议

### 12.1 推荐做法

推荐采用：

- 内容结构学习 OpenClaw
- 技术实现复用 wunder 现有 `web/` 静态站能力
- 文档源统一落到 `docs/静态站文档/`
- 发布路径固定为 `/docs/`
- 页面 layout、frontmatter、Hub 页与 FAQ 组织方式尽量贴近参考项目

这是当前成本最低、风险最小、最符合 wunder 现有代码结构的方案。

### 12.2 不推荐做法

当前阶段不推荐：

1. 直接把整个 `docs/` 根目录公开成文档站
2. 直接把 `docs/方案/` 当正式文档发布
3. 在内容尚未整理前直接引入 Mintlify 并开始迁移
4. 把文档路由挂到 `/wunder/docs/` 且不调整鉴权规则

## 13. 下一步执行建议

如果按本方案推进，下一步应直接做三件事：

1. 先创建 `docs/静态站文档/zh-CN/` 与首批页面清单。
2. 再写 `scripts/build_docs_site.py` 与 `web/docs/` 的静态文档站壳。
3. 最后将现有系统/设计/API 文档逐步拆分迁移，而不是一次性重写全部内容。

这样可以先让 wunder 拥有一个“可访问、可维护、可扩展”的正式文档系统，再继续细化内容与外部站点能力。
