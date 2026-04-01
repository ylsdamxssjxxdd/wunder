# 人员 OSINT 查询工作流

## 语音通知

```bash
curl -s -X POST http://localhost:8888/notify \
  -H "Content-Type: application/json" \
  -d '{"message": "正在运行 情报收集 技能中的 PeopleLookup 工作流以研究个人"}' \
  > /dev/null 2>&1 &
```

正在运行 **PeopleLookup** 工作流（技能：**情报收集**）以研究个人...

**目的：**在授权的专业场景下，对个人进行合规的开源情报收集。

**授权要求：**必须有明确授权、范围定义清晰、法律合规确认。

---

## 阶段 1：授权与范围

**开始前必须核验：**
- [ ] 客户或授权方明确授权
- [ ] 范围定义清晰（目标、信息类型、用途）
- [ ] 法律合规确认（FCRA、GDPR、CCPA、反跟踪法规）
- [ ] 授权文件已记录

**若任何项未勾选，立即停止。**

---

## 来源参考（来自 SOURCES.JSON）

按阶段使用以下来源：

| 调查领域 | 来源 |
|-------------------|---------|
| **身份解析** | Pipl、Spokeo、BeenVerified、TruePeopleSearch、WhitePages、FastPeopleSearch、Radaris、OSINT Industries、That's Them |
| **用户名枚举** | Sherlock、Maigret、WhatsMyName、Namechk、KnowEm、Blackbird |
| **邮箱调查** | Hunter.io、EmailRep、Epieos、GHunt、Holehe、h8mail、HIBP |
| **电话查询** | PhoneInfoga、Truecaller、NumVerify |
| **图片/人脸搜索** | PimEyes、TinEye、Yandex Images、FaceCheck.ID |
| **社交媒体** | Social Searcher、Osintgram、Snapchat Map |
| **学术** | Google Scholar、ResearchGate、ORCID |
| **公共记录** | PACER、CourtListener、州选民登记 |
| **家谱** | FamilySearch、Find A Grave、Ancestry |

---

## 阶段 2：标识符收集

**从已知标识开始：**
- 法定全名（含变体）
- 已知别名或昵称
- 电子邮箱
- 电话号码
- 物理地址
- 社交媒体账号
- 雇主/组织

---

## 阶段 3：职业情报

**LinkedIn 与职业网络：**
- 当前雇主与职位
- 工作履历
- 教育背景
- 技能与背书
- 关系与推荐
- 发布文章/内容

**公司关联：**
- 公司高管检索（OpenCorporates）
- 工商注册（Secretary of State）
- 专利检索（USPTO）
- 职业执照

---

## 阶段 4：公共记录（需授权）

**法律与监管：**
- 法院记录（联邦 PACER、州法院数据库）
- 不动产记录（县级评估系统）
- 商业注册（Secretary of State）
- 职业执照（州级监管机构）
- 选民登记（公开范围内）

**注意：**仅访问授权范围内的记录。

---

## 阶段 5：数字足迹

**域名与邮箱：**
- 域名注册（反向 WHOIS）
- 邮箱变体
- PGP 密钥（密钥服务器）
- Gravatar 等服务

**社交媒体：**
- Facebook、Twitter/X、Instagram、TikTok
- Reddit 历史（公开范围）
- 论坛参与
- 博客作者身份
- 发布内容

---

## 阶段 6：部署研究编队

**并行启动 8 名研究代理实现全面覆盖：**

```typescript
// 身份解析 — Pipl, Spokeo, BeenVerified, TruePeopleSearch, WhitePages, FastPeopleSearch, Radaris
Task({ subagent_type: "PerplexityResearcher", prompt: "在 Pipl、Spokeo、BeenVerified、TruePeopleSearch、WhitePages、FastPeopleSearch 与 Radaris 中检索 [name] 身份记录，交叉核验地址、电话与已知关联人。" })

// 职业背景 — LinkedIn, OpenCorporates, USPTO
Task({ subagent_type: "ClaudeResearcher", prompt: "通过 LinkedIn、OpenCorporates（高管记录）与 USPTO（专利检索）研究 [name] 职业背景，梳理职业履历、商业关联与资质。" })

// 用户名与邮箱枚举 — Sherlock, Maigret, WhatsMyName, Hunter.io, Epieos, Holehe, HIBP
Task({ subagent_type: "GeminiResearcher", prompt: "使用 Sherlock、Maigret 与 WhatsMyName 枚举 [name] 可能使用的用户名；通过 Hunter.io、Epieos、Holehe 与 HIBP 检查邮箱与泄露情况。" })

// 社交媒体深挖 — Social Searcher, Osintgram, 平台内搜索
Task({ subagent_type: "PerplexityResearcher", prompt: "使用 Social Searcher 建立 [name] 的社媒画像，检查 Facebook、Twitter/X、Instagram、TikTok、Reddit 与论坛，提取帖子、关系与活动模式。" })

// 公共记录与法律 — PACER, CourtListener, 州选民登记
Task({ subagent_type: "ClaudeResearcher", prompt: "检索 PACER（联邦法院）与 CourtListener 中涉及 [name] 的法律记录，并查看州选民登记与县级不动产记录。" })

// 图片与人脸搜索 — PimEyes, TinEye, Yandex Images, FaceCheck.ID
Task({ subagent_type: "GeminiResearcher", prompt: "使用 PimEyes、TinEye、Yandex Images 与 FaceCheck.ID 对 [name] 进行以图搜图与人脸匹配，核验跨平台照片一致性。" })

// 学术与出版物 — Google Scholar, ResearchGate, ORCID
Task({ subagent_type: "GrokResearcher", prompt: "在 Google Scholar、ResearchGate 与 ORCID 中检索 [name] 学术发表，核验教育与研究产出。" })

// 资质核验与交叉验证
Task({ subagent_type: "GrokResearcher", prompt: "核验 [name] 的教育、证书与任职主张（大学名录、颁证机构、公司记录），标注与其他来源不一致之处。" })
```

---

## 阶段 7：验证与记录

**交叉验证发现：**
- 每个主张至少多来源支持
- 标注置信度
- 调查矛盾点

**报告结构：**
- 执行摘要
- 目标画像
- 已验证信息
- 未验证主张
- 参考来源
- 使用方法

---

## 伦理护栏

**禁止：**
- 预设身份或冒充
- 访问私有账户
- 购买非法数据
- 社工联系人
- 违反隐私法规

**必须：**
- 留存授权文件
- 遵守范围限制
- 带元数据归档
- 仅使用合规来源

---

**参考：**完整来源目录见 `SOURCES.JSON`，遗留工具详见 `PeopleTools.md`。
