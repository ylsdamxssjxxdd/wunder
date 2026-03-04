# 实体 OSINT 查询工作流

## 语音通知

```bash
curl -s -X POST http://localhost:8888/notify \
  -H "Content-Type: application/json" \
  -d '{"message": "正在运行 情报收集 技能中的 EntityLookup 工作流以调查实体"}' \
  > /dev/null 2>&1 &
```

正在运行 **EntityLookup** 工作流（技能：**情报收集**）以调查实体...

**目的：**对域名、IP、基础设施与威胁实体进行技术情报收集。

**授权要求：**必须有明确授权、范围定义清晰、法律合规确认。

**说明：**“实体”指域名、IP、基础设施、威胁组织等，并非个人。

---

## 阶段 1：实体分类

**实体类型：**
1. **域名** — company.com、subdomain.company.com
2. **IP 地址** — 单个 IP 或 CIDR 段
3. **ASN** — 自治系统号
4. **URL** — 具体网页地址
5. **文件哈希** — MD5、SHA1、SHA256
6. **威胁组织** — 已知恶意组织
7. **基础设施** — C2 服务器、僵尸网络

**提取基础信息：**
- 主标识符
- 关联标识符
- 初始信誉/上下文

---

## 来源参考（来自 SOURCES.JSON）

按阶段使用以下来源：

| 调查领域 | 来源 |
|-------------------|---------|
| **域名/DNS** | SecurityTrails、DomainTools、crt.sh、DNSDumpster、ViewDNS、Robtex、CertStream |
| **IP 信誉** | Shodan、Censys、AbuseIPDB、GreyNoise、BinaryEdge、ZoomEye、Criminal IP、IPinfo |
| **恶意软件分析** | VirusTotal、Hybrid Analysis、ANY.RUN、MalwareBazaar、URLhaus、URLScan.io |
| **漏洞** | NVD、CVE、Exploit-DB、CISA KEV |
| **威胁情报** | Pulsedive、IBM X-Force、Cisco Talos、AlienVault OTX、ThreatFox |
| **暗网/泄露** | Ahmia、HIBP、Intelligence X、DeHashed |
| **框架** | MITRE ATT&CK、D3FEND、ATLAS |
| **僵尸网络/C2** | Feodo Tracker、SSL Blacklist |
| **政府机构** | CISA、UK NCSC、ENISA |

---

## 阶段 2：域名与 URL 情报

**域名分析：**
- WHOIS 查询（注册人、日期、NS）
- DNS 记录（A、AAAA、MX、NS、TXT、CNAME）
- 子域枚举（crt.sh、subfinder、amass）
- 历史 DNS（SecurityTrails、Wayback）

**URL 分析：**
- URLScan.io（截图、技术、重定向）
- VirusTotal（信誉、扫描结果）
- Web 技术识别（Wappalyzer、BuiltWith）

---

## 阶段 3：IP 情报

**地理定位与归属：**
- IPinfo（位置、ASN、组织）
- Hurricane Electric BGP Toolkit（路由、对等）
- RIPE Stat（网络统计）

**信誉：**
- AbuseIPDB（滥用报告、置信度）
- AlienVault OTX（威胁情报）
- 黑名单检查（MXToolbox）

**服务发现：**
- Shodan（端口、服务、漏洞）
- Censys（证书、协议）

---

## 阶段 4：威胁情报（研究代理）

**并行部署 8 名研究代理，按来源定向提示词：**

```typescript
// 恶意软件分析 — VirusTotal, Hybrid Analysis, ANY.RUN, MalwareBazaar, URLhaus, URLScan.io
Task({ subagent_type: "PerplexityResearcher", prompt: "在 VirusTotal、Hybrid Analysis、ANY.RUN、MalwareBazaar、URLhaus 与 URLScan.io 中检索与 [entity] 相关的恶意样本，报告检出率、家族与行为指标。" })

// IP/域名信誉 — Shodan, Censys, AbuseIPDB, GreyNoise, BinaryEdge, Criminal IP
Task({ subagent_type: "PerplexityResearcher", prompt: "通过 AbuseIPDB（滥用报告）、GreyNoise（扫描分类）、Shodan（暴露服务）、Censys（证书）、BinaryEdge 与 Criminal IP 检查 [entity] 信誉，报告置信度与历史标记。" })

// 威胁组织画像 — MITRE ATT&CK, Pulsedive, IBM X-Force, AlienVault OTX
Task({ subagent_type: "ClaudeResearcher", prompt: "使用 MITRE ATT&CK、Pulsedive、IBM X-Force Exchange 与 AlienVault OTX 对与 [entity] 关联的威胁组织画像建模，整理 TTP、相关 IOC 与活动时间线。" })

// 漏洞与利用情报 — NVD, CVE, Exploit-DB, CISA KEV
Task({ subagent_type: "ClaudeResearcher", prompt: "检索 NVD、CVE、Exploit-DB 与 CISA Known Exploited Vulnerabilities 中与 [entity] 相关的漏洞，评估可利用性与是否被在野利用。" })

// C2 与僵尸网络检测 — Feodo Tracker, SSL Blacklist, Cisco Talos, ThreatFox
Task({ subagent_type: "GeminiResearcher", prompt: "将 [entity] 与 Feodo Tracker（C2 服务器）、SSL Blacklist、Cisco Talos 情报与 ThreatFox 进行比对，识别 C2 指标、僵尸网络参与与已知恶意基础设施。" })

// 基础设施关系图谱 — SecurityTrails, DomainTools, Robtex, ViewDNS
Task({ subagent_type: "GeminiResearcher", prompt: "使用 SecurityTrails（历史 DNS）、DomainTools（WHOIS）、Robtex（网络图谱）与 ViewDNS（反向查询）建立 [entity] 基础设施关系，识别共宿域名与共享基础设施。" })

// 暗网与泄露暴露 — Ahmia, HIBP, Intelligence X, DeHashed
Task({ subagent_type: "GrokResearcher", prompt: "在 Ahmia（Tor）、HIBP、Intelligence X 与 DeHashed 中检索 [entity] 的暗网/泄露暴露，报告泄露时间、数据类型与地下论坛提及。" })

// 归因核验与置信度评估
Task({ subagent_type: "GrokResearcher", prompt: "核验 [entity] 的 IOC 主张：区分活跃/历史/误报。交叉参考 CISA、UK NCSC 与 ENISA 的警报/公告，并给出证据权重的置信度评估。" })
```

---

## 阶段 5：网络基础设施

**网络映射：**
- ASN 与网络段
- 托管服务商
- BGP 路由信息
- Traceroute 分析

**云环境识别：**
- AWS、Azure、GCP IP 段匹配
- 云存储枚举（需授权）
- CDN 识别

---

## 阶段 6：邮件基础设施

**MX 分析：**
- 邮件服务器识别
- 邮件服务商检测
- 安全记录（SPF、DMARC、DKIM）
- 黑名单状态

---

## 阶段 7：暗网情报（研究代理）

**并行部署 6 名研究代理，按来源定向提示词：**

```typescript
// Paste 站点与泄露数据 — HIBP, Intelligence X, DeHashed
Task({ subagent_type: "PerplexityResearcher", prompt: "在 HIBP、Intelligence X 与 DeHashed 中检索 [entity] 的 Paste/泄露记录，报告泄露日期、数据类型与暴露范围。" })
Task({ subagent_type: "PerplexityResearcher", prompt: "检查 Intelligence X 历史搜索记录：归档 paste、泄露文档与被删除页面的缓存内容。" })

// 勒索与地下论坛 — Ahmia、泄露站监测
Task({ subagent_type: "ClaudeResearcher", prompt: "检查勒索泄露站与 Ahmia（Tor 搜索）中是否存在 [entity]，搜索数据倾倒、勒索公告与受害者列表。" })
Task({ subagent_type: "ClaudeResearcher", prompt: "搜索地下论坛对 [entity] 的提及：访问代理贩卖、漏洞讨论与凭证交易。" })

// 通讯平台与核验
Task({ subagent_type: "GeminiResearcher", prompt: "在 Telegram 频道与 Discord 服务器中搜索 [entity] 提及，关注已知威胁组织通讯渠道与数据交易群。" })
Task({ subagent_type: "GrokResearcher", prompt: "核验 [entity] 的暗网暴露主张，与 Intelligence X 与 HIBP 交叉验证，分类为已确认/未验证/可能误报。" })
```

---

## 阶段 8：关联与跳转分析

**关系发现：**
- 共享 IP 的域名
- 共享注册人的域名
- 证书关联
- ASN 关联

**跳转点：**
- WHOIS 邮箱 -> 其他域名
- IP 地址 -> 共宿域名
- 域名服务器 -> 全部托管域名
- 证书细节 -> 类似证书

**时间线构建：**
- 注册时间
- 首次出现在威胁情报
- 基础设施变更
- 所有权变更

---

## 阶段 9：分析与报告

**威胁分类：**
- 合法 / 可疑 / 恶意 / 被入侵 / Sinkholed

**置信度等级：**
- 高：多来源独立确认
- 中：部分支持证据
- 低：推测或单一来源

**报告结构：**
1. 实体画像
2. 技术基础设施
3. 声誉与情报
4. 关系与连接
5. 威胁评估
6. 时间线
7. 风险评估
8. 建议
9. IoC（域名、IP、哈希）

---

## 清单

- [ ] 授权已核验
- [ ] 实体已分类
- [ ] WHOIS/DNS 完成
- [ ] IP 情报已收集
- [ ] 威胁情报已查阅
- [ ] VirusTotal 已检索
- [ ] 历史数据已回顾
- [ ] 关系已映射
- [ ] 风险评分已赋值
- [ ] 报告已起草

---

**参考：**完整来源目录见 `SOURCES.JSON`，遗留工具详见 `EntityTools.md`。
