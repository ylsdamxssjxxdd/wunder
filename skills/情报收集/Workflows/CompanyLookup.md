# 公司 OSINT 查询工作流

## 语音通知

```bash
curl -s -X POST http://localhost:8888/notify \
  -H "Content-Type: application/json" \
  -d '{"message": "正在运行 情报收集 技能中的 CompanyLookup 工作流以研究公司"}' \
  > /dev/null 2>&1 &
```

正在运行 **CompanyLookup** 工作流（技能：**情报收集**）以研究公司...

**目的：**在授权范围内进行全面商业情报收集，用于研究、尽调或安全评估。

**授权要求：**必须有明确授权、范围定义清晰、法律合规确认。

---

## 阶段 1：授权与范围

**开始前必须核验：**
- [ ] 客户明确授权
- [ ] 范围清晰（目标公司、信息类型、用途）
- [ ] 法律合规确认
- [ ] 已在合作文件中记录

**若任何项未勾选，立即停止。**

---

## 来源参考（来自 SOURCES.JSON）

按阶段使用以下来源：

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
| **政府合同** | USAspending、GovTribe |
| **制裁** | OFAC、EU Sanctions、OpenSanctions |
| **公司控股结构** | OpenOwnership、GLEIF LEI |
| **初创/VC** | Dealroom、Tracxn、Owler、Wellfound |

---

## 阶段 2：实体识别

**收集初始标识：**
- 法定公司名称（含 DBA）
- 已知域名
- 已知人员（创始人、管理层）
- 地理位置
- 行业/赛道
- 公司结构

---

## 阶段 3：工商注册研究

**公司登记：**
- Secretary of State 注册（相关州）
- 联邦登记（如适用 SEC）
- 外地资质登记
- DBA/虚拟名称注册

**监管登记：**
- 行业许可
- 专业认证
- 证券相关登记

---

## 阶段 4：域名与数字资产

**域名枚举（7 种技术）：**
1. 证书透明度日志（crt.sh）
2. DNS 枚举（subfinder, amass）
3. 搜索引擎发现
4. 社媒简介链接
5. 工商/注册网站字段
6. WHOIS 反向查询
7. 相关 TLD 检查

**详细域名优先协议见 `CompanyDueDiligence.md`。**

---

## 阶段 5：技术基础设施

**对每个发现域名执行：**
- DNS 记录（A、MX、TXT、NS）
- IP 解析与地理定位
- 托管服务商识别
- SSL/TLS 证书分析
- 技术栈画像（BuiltWith、Wappalyzer）
- 安全姿态（SPF、DKIM、DMARC）

---

## 阶段 6：部署研究编队

**并行启动 10 名研究代理，按来源定向提示词：**

```typescript
// 工商注册 — OpenCorporates, SEC EDGAR, Companies House, SAM.gov
Task({ subagent_type: "PerplexityResearcher", prompt: "在 OpenCorporates、SEC EDGAR 与 Companies House 中检索 [company] 的工商注册记录，并检查 SAM.gov 的政府承包商身份。核验法定实体名称、司法辖区、状态与历史申报。" })

// 领导层与关键人员 — LinkedIn, ZoomInfo, Apollo, RocketReach
Task({ subagent_type: "ClaudeResearcher", prompt: "通过 LinkedIn、ZoomInfo、Apollo 与 RocketReach 研究 [company] 的创始人和高管，梳理职业履历、董事会任职与专业资质。" })

// 财务情报 — Crunchbase, PitchBook, D&B, AlphaSense
Task({ subagent_type: "ClaudeResearcher", prompt: "通过 Crunchbase、PitchBook、Dun & Bradstreet 与 AlphaSense 研究 [company] 的融资历史与财务健康度，梳理融资轮次、投资者、收入信号与信用评级。" })

// 法律与监管 — PACER, CourtListener, UniCourt
Task({ subagent_type: "GrokResearcher", prompt: "在 PACER（联邦）、CourtListener 与 UniCourt 中检索涉及 [company] 的法律程序，关注监管处罚、诉讼与合规问题。" })

// 专利与知识产权 — USPTO, Google Patents, Espacenet, Lens.org
Task({ subagent_type: "ClaudeResearcher", prompt: "在 USPTO、Google Patents、Espacenet 与 Lens.org 中检索 [company] 的专利与知识产权申请，评估创新储备与技术护城河。" })

// 技术画像与基础设施 — BuiltWith, Wappalyzer, Netcraft
Task({ subagent_type: "GeminiResearcher", prompt: "使用 BuiltWith、Wappalyzer 与 Netcraft 梳理 [company] 的技术栈，识别框架、分析工具、CDN、托管与第三方集成。" })

// 媒体与新闻覆盖 — GDELT, MediaCloud, Google News
Task({ subagent_type: "PerplexityResearcher", prompt: "通过 GDELT、MediaCloud 与 Google News 分析 [company] 的媒体报道，区分自媒体/广告与第三方报道，跟踪情绪变化。" })

// 竞品情报 — SimilarWeb, SEMrush, Owler
Task({ subagent_type: "GeminiResearcher", prompt: "使用 SimilarWeb（流量）、SEMrush（SEO/广告）与 Owler（竞品跟踪）建立 [company] 的竞争格局，识别市场地位与主要对手。" })

// 制裁与合规 — OFAC, EU Sanctions, OpenSanctions
Task({ subagent_type: "GrokResearcher", prompt: "将 [company] 与 OFAC SDN、EU Consolidated Sanctions 及 OpenSanctions 数据库进行匹配，核验是否存在制裁、出口管制或禁入记录。" })

// 公司控股结构与 VC — OpenOwnership, GLEIF LEI, Dealroom, Tracxn, Wellfound
Task({ subagent_type: "PerplexityResearcher", prompt: "通过 OpenOwnership 与 GLEIF LEI 数据库梳理 [company] 的股权结构，并在 Dealroom、Tracxn 与 Wellfound 中核对投融资历史与股权结构线索。" })
```

---

## 阶段 7：情报综合

**汇总发现：**
- 合法性与真实性指标
- 领导层可信度评估
- 财务健康信号
- 监管合规状态
- 口碑与声誉分析
- 风险信号汇总

**报告结构：**
- 执行摘要
- 公司画像
- 领导层分析
- 财务评估
- 监管状态
- 风险评估
- 参考来源

---

## 质量门槛

**报告定稿前：**
- [ ] 所有域名已发现并分析
- [ ] 工商注册已核验
- [ ] 领导层背景已调查
- [ ] 多来源验证（每条主张 3+ 来源）
- [ ] 风险信号已追踪

---

**相关工作流：**
- `CompanyDueDiligence.md` - 投资级 5 阶段尽调
- **参考：**完整来源目录见 `SOURCES.JSON`，遗留工具详见 `CompanyTools.md`。
