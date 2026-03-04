# 公司投资尽职调查工作流

## 语音通知

```bash
curl -s -X POST http://localhost:8888/notify \
  -H "Content-Type: application/json" \
  -d '{"message": "正在运行 情报收集 技能中的 CompanyDueDiligence 工作流以进行投资核查"}' \
  > /dev/null 2>&1 &
```

正在运行 **CompanyDueDiligence** 工作流（技能：**情报收集**）以进行投资核查...

**目的：**结合域名优先 OSINT、技术侦察、多来源研究与投资风险评估的 5 阶段全面尽调。

**授权要求：**仅用于已授权的投资核查与商业情报场景。

---

## 来源参考（来自 SOURCES.JSON）

用于投资核查的公司 + 威胁情报来源组合：

| 调查领域 | 来源 |
|-------------------|---------|
| **工商注册** | OpenCorporates、SEC EDGAR、Companies House、SAM.gov |
| **财务情报** | Crunchbase、PitchBook、D&B、AlphaSense |
| **员工情报** | LinkedIn、ZoomInfo、Apollo、RocketReach、Hunter.io |
| **法律/诉讼** | PACER、CourtListener、UniCourt |
| **专利/IP** | USPTO、Google Patents、Espacenet、Lens.org |
| **技术画像** | BuiltWith、Wappalyzer、Netcraft |
| **竞品** | SimilarWeb、SEMrush |
| **新闻/媒体** | GDELT、MediaCloud、Google News |
| **制裁** | OFAC、EU Sanctions、OpenSanctions |
| **公司控股结构** | OpenOwnership、GLEIF LEI |
| **域名/DNS** | SecurityTrails、DomainTools、crt.sh、DNSDumpster、ViewDNS |
| **IP/基础设施** | Shodan、Censys、AbuseIPDB、GreyNoise |
| **威胁情报** | VirusTotal、URLScan.io、Pulsedive |
| **暗网/泄露** | HIBP、Intelligence X、DeHashed |
| **初创/VC** | Dealroom、Tracxn、Owler、Wellfound |
| **政府合同** | USAspending、GovTribe |

---

## 关键设计：域名优先协议（DOMAIN-FIRST）

**域名发现是强制的第一步，并会阻断后续阶段。**

此设计可避免遗漏投资者门户或替代 TLD（如 .partners、.capital、.fund）导致的情报缺口。

---

## 5 阶段概览

```
阶段 1：域名发现（阻断）
    [质量门槛：95%+ 置信度覆盖全部域名]
阶段 2：技术侦察
    [质量门槛：全部域名/IP/ASN 枚举完成]
阶段 3：全面研究（32+ 代理）
    [质量门槛：每条主张至少 3 个来源]
阶段 4：投资核查
    [质量门槛：所有风险信号已调查]
阶段 5：综合与建议
```

---

## 阶段 1：域名发现（阻断）

**并行执行 7 种枚举技术：**

1. **证书透明度：**crt.sh、certspotter
2. **DNS 枚举：**subfinder、amass、assetfinder
3. **搜索引擎发现：**委派给 Research Skill
4. **社媒链接：**提取所有个人/公司主页链接
5. **工商/注册信息：**申报文件中的网站字段
6. **WHOIS 反向查询：**注册人邮箱/名称关联
7. **相关 TLD 发现：**检查 .com、.net、.partners、.capital、.fund

**质量门槛验证：**
- [ ] 7 种技术全部执行
- [ ] 找到投资者面向网站（或高度确信不存在）
- [ ] 团队/关于页面已发现
- [ ] 域名覆盖置信度 ≥ 95%

**未通过质量门槛不得进入下一阶段。**

---

## 阶段 2：技术侦察

**部署渗透测试编队（每个域名一个）：**

对每个发现域名：
- DNS 记录（A、AAAA、MX、TXT、NS、SOA、CNAME）
- SSL/TLS 证书分析
- IP 解析与 ASN 识别
- Web 技术指纹识别
- 安全姿态评估

**补充 IP 级侦察：**
- 地理定位与托管服务商
- 反向 DNS 查询
- 网络段归属识别

---

## 阶段 3：全面研究（32+ 代理）

**并行部署研究编队（超时 10 分钟）：**

**商业合法性（8 名代理）：**
- 实体注册核验 — OpenCorporates、SEC EDGAR、Companies House、SAM.gov
- 监管合规检查 — OFAC、EU Sanctions、OpenSanctions
- 领导层背景 — LinkedIn、ZoomInfo、Apollo、RocketReach
- 财务情报 — Crunchbase、PitchBook、D&B、AlphaSense

```typescript
Task({ subagent_type: "PerplexityResearcher", prompt: "在 OpenCorporates、SEC EDGAR、Companies House 与 SAM.gov 中检索 [company] 的工商注册，核验实体状态、司法辖区、管理人员与申报历史。" })
Task({ subagent_type: "GrokResearcher", prompt: "将 [company] 与 OFAC SDN、EU Consolidated Sanctions、OpenSanctions 及禁入名单比对，报告监管处罚或合规违规。" })
Task({ subagent_type: "ClaudeResearcher", prompt: "通过 LinkedIn、ZoomInfo、Apollo 与 RocketReach 研究 [company] 领导层，梳理职业履历、董事席位、资质与潜在利益冲突。" })
Task({ subagent_type: "ClaudeResearcher", prompt: "通过 Crunchbase（融资轮次）、PitchBook（估值）、D&B（信用评级）与 AlphaSense（财报/披露）评估 [company] 财务健康度，梳理收入信号与财务轨迹。" })
Task({ subagent_type: "PerplexityResearcher", prompt: "检索 PACER、CourtListener 与 UniCourt 中涉及 [company] 的法律程序，关注诉讼、监管行动、破产申请与知识产权纠纷。" })
Task({ subagent_type: "ClaudeResearcher", prompt: "在 USPTO、Google Patents、Espacenet 与 Lens.org 中检索 [company] 的专利/IP 组合，评估专利覆盖度、引用量与技术可防御性。" })
Task({ subagent_type: "GrokResearcher", prompt: "通过 OpenOwnership 与 GLEIF LEI 绘制 [company] 的股权结构，识别受益所有人、子公司关系与控股链条。" })
Task({ subagent_type: "PerplexityResearcher", prompt: "通过 USAspending 与 GovTribe 检索 [company] 的政府合同历史，报告合同金额、机构与履约评分。" })
```

**声誉与市场（8 名代理）：**
- 媒体覆盖分析（自传播 vs 付费）— GDELT、MediaCloud、Google News
- 客户口碑评估 — G2、Trustpilot、BBB
- 竞争格局 — SimilarWeb、SEMrush、Owler
- 市场机会验证 — Dealroom、Tracxn、Wellfound

```typescript
Task({ subagent_type: "PerplexityResearcher", prompt: "通过 GDELT、MediaCloud 与 Google News 分析 [company] 媒体覆盖，区分自传播/广告与第三方报道，跟踪情绪与报道量变化。" })
Task({ subagent_type: "GeminiResearcher", prompt: "通过 G2、Trustpilot、BBB 与应用商店评价评估 [company] 客户情绪，识别重复投诉与满意度模式。" })
Task({ subagent_type: "GeminiResearcher", prompt: "使用 SimilarWeb（流量/互动）、SEMrush（搜索可见度）、Owler（竞品跟踪）建立 [company] 竞争格局，识别市场位置与竞争威胁。" })
Task({ subagent_type: "PerplexityResearcher", prompt: "通过 Dealroom、Tracxn 与 Wellfound 研究 [company] 的市场机会，评估 TAM、行业融资趋势与可比退出/估值。" })
Task({ subagent_type: "ClaudeResearcher", prompt: "搜索 [company] 的行业认可与奖项，检查会议出席、分析师提及与思想领导力指标。" })
Task({ subagent_type: "GeminiResearcher", prompt: "通过 Glassdoor、Blind 与 LinkedIn 帖子分析 [company] 员工情绪，评估招聘速度、离职信号与文化健康度。" })
Task({ subagent_type: "GrokResearcher", prompt: "研究 [company] 的历史背景（转型、改名、创始人履历、前身实体），使用 Wayback Machine 跟踪官网演进。" })
Task({ subagent_type: "ClaudeResearcher", prompt: "通过 BuiltWith、Wappalyzer 与 Netcraft 画像 [company] 技术栈与基础设施，评估技术成熟度、云采用情况与安全姿态。" })
```

**验证（8 名代理）：**
- 主张验证（收入、客户、合作）
- 资质验证（教育、证书）
- 跨来源一致性核验

```typescript
Task({ subagent_type: "GrokResearcher", prompt: "交叉核验 [company] 的收入主张（SEC 披露、Crunchbase、新闻稿、LinkedIn/ZoomInfo 员工规模），标注差异。" })
Task({ subagent_type: "GrokResearcher", prompt: "核验 [company] 的客户主张：查找案例、官网客户 Logo、独立客户佐证，确认被点名客户是否认可合作关系。" })
Task({ subagent_type: "ClaudeResearcher", prompt: "核验 [company] 的合作伙伴主张：检查合作方目录、联合新闻稿与集成市场，区分“客户/合作/集成”。" })
Task({ subagent_type: "GrokResearcher", prompt: "核验 [company] 领导层的教育与资质：大学校友名录、证书数据库、职业执照登记。" })
Task({ subagent_type: "PerplexityResearcher", prompt: "将 [company] 的所有主张与独立来源交叉比对，检查官网、LinkedIn、Crunchbase 与 SEC 披露之间的不一致。" })
Task({ subagent_type: "GeminiResearcher", prompt: "在 HIBP、Intelligence X 与 DeHashed 中检索 [company] 域名泄露暴露，关注数据泄露、凭证泄漏与暗网提及。" })
Task({ subagent_type: "PerplexityResearcher", prompt: "将 [company] 域名与 VirusTotal、URLScan.io、AbuseIPDB 进行比对，评估是否存在恶意托管、钓鱼指标或滥用报告。" })
Task({ subagent_type: "GeminiResearcher", prompt: "通过 SecurityTrails、crt.sh 与 DNSDumpster 枚举 [company] 的全部域名与子域，识别影子 IT、遗忘资产与基础设施范围。" })
```

**专项（8 名代理）：**
- 行业深挖与 IP 评估
- 威胁情报叠加

```typescript
Task({ subagent_type: "ClaudeResearcher", prompt: "深度研究 [company] 所在行业：市场规模、增长率、监管环境与影响其商业模式的关键趋势。" })
Task({ subagent_type: "GeminiResearcher", prompt: "通过 Shodan（暴露服务）、Censys（证书）与 GreyNoise（扫描活动）评估 [company] 的技术基础设施安全姿态。" })
Task({ subagent_type: "PerplexityResearcher", prompt: "研究 [company] 的开源贡献、GitHub 活跃度与开发者社区参与，评估技术人才信号。" })
Task({ subagent_type: "GrokResearcher", prompt: "分析 [company] 的社媒表现（Twitter/X、LinkedIn 公司页、YouTube），评估粉丝质量、互动率与内容策略。" })
Task({ subagent_type: "ClaudeResearcher", prompt: "研究 [company] 的 ESG 画像：环保承诺、社会影响项目、治理结构，识别漂绿或表面化 DEI 风险。" })
Task({ subagent_type: "GeminiResearcher", prompt: "在 Pulsedive、Cisco Talos 与 AlienVault OTX 中检索 [company] 的威胁情报关联，报告其基础设施是否出现在 IOC 订阅中。" })
Task({ subagent_type: "PerplexityResearcher", prompt: "通过 LinkedIn Jobs、Indeed 与 Glassdoor 分析 [company] 的招聘模式，推断战略方向、燃烧率与增长领域。" })
Task({ subagent_type: "GrokResearcher", prompt: "对 [company] 做最终风险扫查：覆盖尚未涉及的来源，检查 BBB 投诉、FTC 执法数据库与州检察长行动。" })
```

---

## 阶段 4：投资核查

**合法性评估框架：**

**强信号：**
- 活跃的工商登记
- SEC 披露（如适用）
- 具名且资质明确的董事会
- 可获得审计财报
- 行业协会成员资格

**警示信号：**
- 成立多年的公司却线上存在感极弱
- 运营多年但无客户证言
- 宣传性内容远多于第三方报道

**红旗：**
- 实体已注销或不活跃
- 监管处罚记录
- 资质或主张虚假

**风险评分（0-100）：**
- 商业风险（0-10）
- 监管风险（0-10）
- 团队风险（0-10）
- 透明度风险（0-10）
- 市场风险（0-10）

**分数解释：**
- 0-20：低风险 — 可推进
- 21-40：中等 — 附条件推进
- 41-60：高风险 — 建议拒绝
- 61-100：严重 — 建议避免

---

## 阶段 5：综合与建议

**执行摘要模板：**

```markdown
**目标：** [company name]
**风险评估：** [LOW/MODERATE/HIGH/CRITICAL]
**建议：** [PROCEED/PROCEED WITH CONDITIONS/DECLINE/AVOID]

### 关键发现（Top 5）
1. [Finding]
2. [Finding]
...

### 关键红旗
- [If any]

### 投资优势
1. [Strength]
...

### 建议
[2-3 段建议，包含可执行行动项]
```

---

## 文件组织

```
~/.claude/MEMORY/WORK/$(jq -r '.work_dir' ~/.claude/MEMORY/STATE/current-work.json)/YYYY-MM-DD-HHMMSS_due-diligence-[company]/
  phase1-domains.md
  phase2-technical.md
  phase3-research.md
  phase4-vetting.md
  phase5-report.md

~/.claude/History/research/YYYY-MM/[company]-due-diligence/
  comprehensive-report.md
  risk-assessment.md
  metadata.json
```

---

## 伦理合规

- 仅限开源情报
- 禁止未授权访问
- 禁止社会工程
- 尊重隐私与 ToS
- 必须满足法律合规
- 授权需留档

---

**参考：**完整来源目录见 `SOURCES.JSON`，遗留工具详见 `CompanyTools.md`。
