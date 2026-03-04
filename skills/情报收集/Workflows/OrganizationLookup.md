# 组织 OSINT 查询工作流

## 语音通知

```bash
curl -s -X POST http://localhost:8888/notify \
  -H "Content-Type: application/json" \
  -d '{"message": "正在运行 情报收集 技能中的 OrganizationLookup 工作流以研究组织"}' \
  > /dev/null 2>&1 &
```

正在运行 **OrganizationLookup** 工作流（技能：**情报收集**）以研究组织...

**目的：**调查非商业实体——NGO、政府机构、协会、学术机构、非营利组织、基金会、国际组织。与 CompanyLookup 不同，这些实体的注册系统、资金模型、透明度要求与问责结构不同。

**授权要求：**必须有明确授权、范围定义清晰、法律合规确认。

---

## 阶段 1：授权与范围

**开始前必须核验：**
- [ ] 客户或授权方明确授权
- [ ] 范围定义清晰（目标组织、信息类型、用途）
- [ ] 法律合规确认
- [ ] 授权文件已记录

**若任何项未勾选，立即停止。**

---

## 来源参考（来自 SOURCES.JSON）

| 调查领域 | 来源 |
|-------------------|---------|
| **非营利注册** | IRS（990 报告）、GuideStar/Candid、Charity Navigator、州慈善注册 |
| **政府实体** | USAspending、GovTribe、SAM.gov、FOIA 门户、机构网站 |
| **国际组织** | 联合国数据库、OECD、世界银行开放数据、OpenSanctions |
| **学术机构** | NCES（美国教育统计中心）、认证数据库、Google Scholar |
| **企业注册** | OpenCorporates、州 Secretary of State 申报 |
| **领导层/人员** | LinkedIn、ZoomInfo、董事会名单、公开任命记录 |
| **财务透明度** | IRS 990（ProPublica Nonprofit Explorer）、年度报告、基金数据库（Foundation Directory） |
| **法律/监管** | PACER、CourtListener、OFAC、OIG 排除名单 |
| **新闻/媒体** | GDELT、MediaCloud、Google News |
| **数字存在** | BuiltWith、Wappalyzer、Netcraft、SecurityTrails |
| **制裁/合规** | OFAC、EU Sanctions、OpenSanctions、OIG |
| **域名/基础设施** | SecurityTrails、DomainTools、crt.sh、Shodan |

---

## 阶段 2：组织识别

**收集初始标识：**
- 法定名称与常用名称
- 组织类型（非营利、政府、NGO、学术、基金会、协会、国际组织）
- 法域（美国州、国家、国际）
- EIN/税号（美国非营利）
- 使命陈述
- 成立年份
- 已知领导层（执行主任、董事会主席、机构负责人）
- 已知域名与社交媒体

**组织分类：**

| 类型 | 主要注册机构 | 关键财务来源 |
|------|-------------------|----------------------|
| 美国非营利（501c3/4） | IRS、州总检察长、Secretary of State | IRS 990（ProPublica）、年度报告 |
| 政府机构 | SAM.gov、机构组织架构 | USAspending、预算文件 |
| 学术机构 | NCES、认证机构 | IRS 990（若为私立）、捐赠基金报告 |
| 国际 NGO | UN ECOSOC、所在国注册 | 年度报告、捐助披露 |
| 基金会 | IRS（私募基金会 990-PF） | 990-PF 资助清单 |
| 行业协会 | IRS（501c6）、州注册 | IRS 990、会员披露 |

---

## 阶段 3：注册与法律状态

**非营利（美国）：**
- IRS 免税资格核验（IRS Select Check）
- ProPublica Nonprofit Explorer 的 990 报告
- 州慈善注册（州总检察长数据库）
- Secretary of State 企业申报
- GuideStar/Candid 档案
- Charity Navigator 评级

**政府机构：**
- 机构组织架构与领导层
- 法定授权/授权法案
- SAM.gov 实体注册
- 监察长报告
- FOIA 申请历史（MuckRock）

**学术机构：**
- 认证状态（区域/国家认证数据库）
- NCES 数据（招生、毕业率、财务）
- 研究经费（NSF Award Search、NIH Reporter）

**国际组织：**
- UN ECOSOC 咨询地位
- 母国注册
- 国际慈善名录
- OpenSanctions 核查

---

## 阶段 4：领导层与关键人员

**董事会/理事会：**
- 董事成员姓名与关联组织
- 董事会结构（独立性、多样性、专业性）
- 潜在利益冲突
- 与其他组织的交叉任职

**执行层：**
- 执行主任/CEO 背景（LinkedIn、职业履历）
- 薪酬（IRS 990 Part VII）
- 任期与流动情况
- 过往角色与组织

**关键员工：**
- 项目负责人与高级管理层
- 公开专业成果（Google Scholar、ResearchGate）
- 媒体露出与公开发言

---

## 阶段 5：资金与财务透明度

**收入来源（IRS 990）：**
- 项目服务收入
- 政府补助与合同
- 私人捐赠与筹款
- 投资收入
- 服务收费

**支出分析（IRS 990）：**
- 项目支出 vs 行政 vs 筹款
- 项目效率比
- 高管薪酬与预算占比
- 关联方交易

**资助追踪：**
- Foundation Directory Online（获得资助）
- USAspending（政府资助/合同）
- 州级资助数据库
- 捐赠清单（年度报告）

**财务健康指标：**
- 收入趋势（增长/稳定/下降）
- 备付金/净资产比
- 资金来源多样性
- 审计发现（如有）

---

## 阶段 6：数字存在与声誉

**域名与基础设施：**
- 官网分析（BuiltWith、Wappalyzer）
- 域名年龄与注册信息（DomainTools）
- 子域发现（crt.sh、SecurityTrails）
- 邮件基础设施（MX、SPF、DMARC）

**社交媒体：**
- 官方账号（Twitter/X、LinkedIn、Facebook、Instagram、YouTube）
- 粉丝质量与互动
- 内容与使命一致性
- 员工社媒存在

**新闻与媒体覆盖：**
- GDELT 事件监测
- MediaCloud 覆盖分析
- Google News 时间线
- 领导层社论与公开声明

**声誉：**
- GuideStar/Candid 透明度徽章
- Charity Navigator 评级
- BBB Wise Giving Alliance
- 监督机构评估
- 争议或批评历史

---

## 阶段 7：部署研究编队

**并行启动 8 名研究代理，按来源定向提示词：**

```typescript
// 注册与法律状态 — IRS, ProPublica, GuideStar, 州级注册
Task({ subagent_type: "PerplexityResearcher", prompt: "核验 [organization] 的注册与法律状态：IRS Select Check 免税资格、ProPublica Nonprofit Explorer 的 990 报告、GuideStar/Candid 透明度档案，以及州慈善注册数据库。报告 EIN、裁定年份、可抵扣状态与是否被撤销。" })

// 财务分析 — IRS 990, 年度报告, USAspending
Task({ subagent_type: "ClaudeResearcher", prompt: "基于 IRS 990（ProPublica）、年度报告与 USAspending（政府资助）分析 [organization] 财务健康度，计算项目效率比、收入趋势、薪酬分析与资金来源多样性。" })

// 领导层背景 — LinkedIn, ZoomInfo, 公开记录
Task({ subagent_type: "ClaudeResearcher", prompt: "通过 LinkedIn、ZoomInfo 与公开任命记录研究 [organization] 领导层与董事会，梳理高管背景、董事关联、薪酬（990 Part VII）与潜在利益冲突。" })

// 资助与资金来源 — Foundation Directory, USAspending, 捐赠披露
Task({ subagent_type: "PerplexityResearcher", prompt: "通过 Foundation Directory（资助）、USAspending（政府合同）与公开捐赠清单梳理 [organization] 资金来源，识别主要资助方、资助金额与异常资金模式。" })

// 新闻与声誉 — GDELT, MediaCloud, Google News, Charity Navigator
Task({ subagent_type: "GeminiResearcher", prompt: "通过 GDELT、MediaCloud 与 Google News 分析 [organization] 媒体报道，核查 Charity Navigator 评级、BBB Wise Giving Alliance 与 GuideStar 透明度。识别争议、批评或调查报道。" })

// 法律与合规 — PACER, CourtListener, OFAC, OIG
Task({ subagent_type: "GrokResearcher", prompt: "检索 PACER 与 CourtListener 中涉及 [organization] 的法律程序，检查 OFAC 制裁名单、OIG 排除名单与州总检察长执法行动，报告合规问题。" })

// 数字存在与基础设施 — BuiltWith, SecurityTrails, crt.sh, Shodan
Task({ subagent_type: "GeminiResearcher", prompt: "画像 [organization] 数字基础设施：BuiltWith（网站技术）、SecurityTrails（域名/DNS）、crt.sh（子域）、Shodan（暴露服务）。评估安全姿态与数字成熟度。" })

// 使命影响力与项目评估
Task({ subagent_type: "GrokResearcher", prompt: "评估 [organization] 的项目影响与使命有效性，查找独立评估、项目成果数据、学术引用与受益者证言，对比使命陈述与实际活动/支出。" })
```

---

## 阶段 8：综合

**合法性评估：**
- 注册状态（有效、良好）
- 免税资格核验
- 财务透明度（已提交 990、可用审计财报）
- 领导层可信度
- 项目活动与使命一致

**资金透明度：**
- 主要资助方清晰
- 政府资助可追溯
- 收入/支出趋势
- 项目效率（>75% 投入项目 = 良好）
- 关联方交易标记

**影响评估：**
- 可量化项目成果
- 独立评估或审计
- 受益覆盖范围
- 社区声誉

**风险指标：**

| 风险等级 | 指标 |
|-----------|------------|
| **低** | 注册有效、990 透明、评级良好、资金多样、领导层稳定 |
| **中** | 透明度有限、新成立、资金集中、行政成本高 |
| **高** | 资格被撤销、监管行动、财务不透明、领导层流动、使命漂移 |
| **严重** | 命中制裁、欺诈迹象、空壳组织、虚假项目 |

**报告结构：**
1. 组织画像（类型、使命、法域）
2. 注册与法律状态
3. 领导层分析
4. 财务概览（5 年趋势）
5. 资金来源与透明度
6. 项目影响评估
7. 数字存在与安全
8. 声誉与媒体分析
9. 风险评估
10. 建议

---

## 清单

- [ ] 授权已核验
- [ ] 组织类型已分类
- [ ] 注册状态已核验
- [ ] IRS 990 / 财务数据已分析
- [ ] 领导层背景已研究
- [ ] 资金来源已映射
- [ ] 数字存在已画像
- [ ] 新闻/媒体覆盖已分析
- [ ] 制裁/合规已检查
- [ ] 风险评分已赋值
- [ ] 报告已起草

---

**参考：**完整来源目录见 `SOURCES.JSON`。
