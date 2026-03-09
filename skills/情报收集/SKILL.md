---
名称: 情报收集
描述: 结构化 OSINT（开源情报）调查：人员查询、公司情报、投资尽调、实体/威胁情报、域名与子域侦察、组织/机构研究等，并内置授权与伦理框架。适用于：OSINT、尽职调查、背景调查、人物/公司/实体/组织/域名情报、威胁情报、发现 OSINT 资源等场景。
---

## 自定义

**执行前检查用户自定义：**
`~/.claude/PAI/USER/SKILLCUSTOMIZATIONS/情报收集/`

如果该目录存在，加载并应用其中的 PREFERENCES.md、配置或资源，覆盖默认行为。若目录不存在，按技能默认执行。


## 🚨 强制：语音通知（任何动作前必须执行）

**技能被调用后，在做任何事情之前，必须先发送通知。**

1. **发送语音通知**:
   ```bash
   curl -s -X POST http://localhost:8888/notify \
     -H "Content-Type: application/json" \
     -d '{"message": "正在运行 情报收集 技能中的 WORKFLOWNAME 工作流以执行 ACTION"}' \
     > /dev/null 2>&1 &
   ```

2. **输出文字通知**:
   ```
   正在运行 **WorkflowName** 工作流（技能：**情报收集**）以执行 ACTION...
   ```

**这是强制要求。技能被调用时应立刻执行上述 curl 命令。**

# 情报收集（OSINT）技能

面向授权场景的开源情报收集与分析。

---


## 工作流路由

| 调查类型 | 工作流 | 上下文 |
|-------------------|----------|---------|
| 人员查询 | `{{SKILL_ROOT}}/Workflows/PeopleLookup.md` | `{{SKILL_ROOT}}/SOURCES.JSON` |
| 公司查询 | `{{SKILL_ROOT}}/Workflows/CompanyLookup.md` | `{{SKILL_ROOT}}/SOURCES.JSON` |
| 投资尽调 | `{{SKILL_ROOT}}/Workflows/CompanyDueDiligence.md` | `{{SKILL_ROOT}}/SOURCES.JSON` |
| 实体/威胁情报 | `{{SKILL_ROOT}}/Workflows/EntityLookup.md` | `{{SKILL_ROOT}}/SOURCES.JSON` |
| 域名/子域调查 | `{{SKILL_ROOT}}/Workflows/DomainLookup.md` | `{{SKILL_ROOT}}/SOURCES.JSON` |
| 组织/NGO/政府研究 | `{{SKILL_ROOT}}/Workflows/OrganizationLookup.md` | `{{SKILL_ROOT}}/SOURCES.JSON` |
| 发现新 OSINT 资源 | `{{SKILL_ROOT}}/Workflows/DiscoverOSINTSources.md` | `{{SKILL_ROOT}}/SOURCES.JSON` |

---

## 触发示例

**人员 OSINT：**
- “对[人]做 OSINT”“调查[人]”“[人]背景调查”
- “[人]是谁”“查[人]信息”“调查此人”
-> 路由到 `{{SKILL_ROOT}}/Workflows/PeopleLookup.md`

**公司 OSINT：**
- “对[公司]做 OSINT”“调查[公司]”“公司情报”
- “能查到[公司]什么”“调查[公司]”
-> 路由到 `{{SKILL_ROOT}}/Workflows/CompanyLookup.md`

**投资尽调：**
- “对[公司]尽调”“评估[公司]是否靠谱”“[公司]是否合法”
- “评估[公司]”“要不要和[公司]合作”
-> 路由到 `{{SKILL_ROOT}}/Workflows/CompanyDueDiligence.md`

**实体/威胁情报：**
- “调查[实体]”“[实体]威胁情报”“这个是否恶意”
- “研究这个威胁组织”“分析[实体]”“检查这个 IP”
-> 路由到 `{{SKILL_ROOT}}/Workflows/EntityLookup.md`

**域名/子域调查：**
- “调查域名”“检查域名”“子域枚举”
- “对[域名]做域名侦察”“[域名]有哪些子域”
- “DNS 调查”“[域名]证书透明度”
-> 路由到 `{{SKILL_ROOT}}/Workflows/DomainLookup.md`

**组织/NGO/政府：**
- “研究组织”“调查 NGO”“研究机构”
- “[组织]是谁”“调查[非营利]”“研究[政府机构]”
- “关于[协会]我们知道什么”“[机构]背景”
-> 路由到 `{{SKILL_ROOT}}/Workflows/OrganizationLookup.md`

---

## 授权（必需）

**在任何调查前，必须确认：**
- [ ] 已获得客户明确授权
- [ ] 范围定义清晰
- [ ] 已确认合规与法律要求
- [ ] 文档齐备

**若任何项未勾选，立即停止。**详见 `{{SKILL_ROOT}}/EthicalFramework.md`。

---

## 资源索引

| 文件 | 作用 |
|------|---------|
| `{{SKILL_ROOT}}/SOURCES.JSON` | 279 条 OSINT 资源主目录（英文，便于一致引用） |
| `{{SKILL_ROOT}}/SOURCES.md` | 可阅读的资源说明清单（英文，含描述与访问信息） |
| `{{SKILL_ROOT}}/EthicalFramework.md` | 授权、法律与伦理边界 |
| `{{SKILL_ROOT}}/Methodology.md` | 采集方法、验证与报告规范 |
| `{{SKILL_ROOT}}/PeopleTools.md` | 人员搜索/社媒/公共记录（遗留清单，优先用 SOURCES.JSON） |
| `{{SKILL_ROOT}}/CompanyTools.md` | 企业数据库/DNS/技术画像（遗留清单，优先用 SOURCES.JSON） |
| `{{SKILL_ROOT}}/EntityTools.md` | 威胁情报/扫描/恶意软件分析（遗留清单，优先用 SOURCES.JSON） |

---

## 集成

**自动调用技能：**
- **Research Skill** - 并行研究代理部署（必需）
- **Recon Skill** - 技术基础设施侦察

**代理编队规模：**
- 快速查询：4-6 名代理
- 标准调查：8-16 名代理
- 全面尽调：24-32 名代理

**研究者类型：**
| 研究者 | 适用场景 |
|------------|----------|
| PerplexityResearcher | 最新网页数据、社媒、公司动态 |
| ClaudeResearcher | 学术深度、专业背景 |
| GeminiResearcher | 多视角、跨领域关联 |
| GrokResearcher | 反向验证、事实核查 |

---

## 文件组织

**进行中的调查：**
```
~/.claude/MEMORY/WORK/$(jq -r '.work_dir' ~/.claude/MEMORY/STATE/current-work.json)/YYYY-MM-DD-HHMMSS_osint-[target]/
```

**归档报告：**
```
~/.claude/History/research/YYYY-MM/[target]-osint/
```

---

## 伦理护栏

**允许：**仅使用公开来源（网站、社交媒体、公共记录、搜索引擎、历史归档）

**禁止：**私有数据、未授权访问、社工、购买泄露数据、违反平台 ToS

完整要求见 `{{SKILL_ROOT}}/EthicalFramework.md`。

---

**版本：** 3.0（SOURCES.JSON 集成）
**最后更新：** 2026 年 2 月
