# 域名 OSINT 查询工作流

## 语音通知

```bash
curl -s -X POST http://localhost:8888/notify \
  -H "Content-Type: application/json" \
  -d '{"message": "正在运行 情报收集 技能中的 DomainLookup 工作流以调查域名"}' \
  > /dev/null 2>&1 &
```

正在运行 **DomainLookup** 工作流（技能：**情报收集**）以调查域名...

**目的：**专注于域名与子域调查——注册情报、DNS 枚举、子域发现、技术指纹、证书透明度与信誉评估。

**授权要求：**必须有明确授权、范围定义清晰、法律合规确认。

---

## 阶段 1：授权与范围

**开始前必须核验：**
- [ ] 客户或授权方明确授权
- [ ] 范围定义清晰（目标域名、枚举深度、仅被动或含主动）
- [ ] 法律合规确认（如有主动扫描需在授权渗透测试范围内）
- [ ] 授权文件已记录

**若任何项未勾选，立即停止。**

---

## 来源参考（来自 SOURCES.JSON）

| 调查领域 | 来源 |
|-------------------|---------|
| **WHOIS/注册** | DomainTools、WHOIS 数据库、ViewDNS |
| **DNS 枚举** | SecurityTrails、DNSDumpster、Robtex、ViewDNS |
| **子域发现** | crt.sh、SecurityTrails、DNSDumpster、subfinder、amass |
| **技术/托管** | BuiltWith、Wappalyzer、Netcraft、Shodan、Censys |
| **证书透明度** | crt.sh、CertStream、Cert Spotter |
| **信誉/威胁情报** | VirusTotal、URLScan.io、AbuseIPDB、GreyNoise、PhishTank |
| **泄露/暴露** | HIBP、Intelligence X |
| **历史** | SecurityTrails（历史 DNS）、Wayback Machine |

---

## 阶段 2：域名注册情报

**WHOIS 与注册人分析：**
- WHOIS 查询（注册人、组织、邮箱、日期）
- 注册日期与到期日期
- 注册商识别
- 隐私/代理服务识别
- 注册人历史（DomainTools 历史 WHOIS）
- 域名服务器历史

**关键问题：**
- 域名何时注册？
- 注册人信息是否变化？变更次数？
- 是否启用隐私保护？（可能是合规企业，也可能是规避）
- 注册人是否拥有其他域名？（DomainTools/ViewDNS 反向 WHOIS）

---

## 阶段 3：DNS 枚举

**完整 DNS 记录收集：**
- A 记录（IPv4）
- AAAA 记录（IPv6）
- MX 记录（邮件服务器）
- TXT 记录（SPF、DKIM、DMARC、验证令牌）
- NS 记录（权威 DNS）
- CNAME 记录（别名）
- SOA 记录（区域权威）

**历史 DNS — SecurityTrails：**
- 旧 IP 地址
- 旧域名服务器
- 旧 MX 记录
- DNS 变更时间线

**分析：**
- DNS 记录是否指向已知托管商？
- 是否存在悬空 CNAME（子域接管风险）？
- 安全记录是否齐全（SPF、DMARC、DKIM）？
- 是否频繁更换托管？

---

## 阶段 4：子域发现

**执行多种枚举技术：**

1. **证书透明度（crt.sh）** — 历史证书全量
2. **DNS 字典爆破（subfinder）** — 常见子域词表
3. **被动 DNS（SecurityTrails）** — 历史子域记录
4. **DNS 聚合（DNSDumpster）** — 被动情报合并
5. **Amass 被动** — 多来源子域枚举

**对每个发现的子域：**
- 解析到 IP
- 检查是否存活（HTTP 响应）
- 识别托管商
- 记录命名模式（dev、staging、admin、api、vpn、mail）

**质量门槛：**
- [ ] 5 种枚举技术全部执行
- [ ] 结果去重并合并
- [ ] 每个子域已解析并检查状态
- [ ] 子域覆盖置信度 ≥ 95%

---

## 阶段 5：技术与托管指纹

**对主域与关键子域：**

**Web 技术 — BuiltWith、Wappalyzer、Netcraft：**
- Web 框架（React、Angular、Vue 等）
- CMS（WordPress、Drupal 等）
- 分析工具（GA、Mixpanel 等）
- CDN（Cloudflare、Akamai、Fastly）
- 托管商
- 服务器软件（nginx、Apache 等）
- JavaScript 库
- 广告/追踪

**基础设施 — Shodan、Censys：**
- 开放端口与服务
- SSL/TLS 证书细节（签发者、到期、SAN）
- 服务器 Banner 与版本
- 暴露服务的已知漏洞

**IP 情报：**
- 地理定位
- ASN 与网络归属
- 反向 DNS
- 同 IP 其他域名（共享托管识别）

---

## 阶段 6：证书透明度

**crt.sh 分析：**
- 历史上为该域名签发的所有证书
- 通配符证书（*.domain.com）
- SAN（Subject Alternative Name）条目——揭示关联域名
- 证书签发机构（Let’s Encrypt vs 商业 CA）
- 证书时间线（首次签发时间）

**CertStream / Cert Spotter：**
- 实时证书签发监控
- 通过 CT 日志发现新子域

**分析：**
- SAN 是否暴露隐藏子域或关联域名？
- 首次证书签发时间（域名年龄指标）
- 是否存在非显性域名证书（影子 IT）

---

## 阶段 7：信誉与威胁情报

**VirusTotal：**
- 域名扫描结果
- 关联恶意检测
- 社区评论与评分
- URL 扫描历史
- 关联域名/IP 标记

**URLScan.io：**
- 页面实时截图
- 技术识别
- 重定向链
- 第三方请求

**AbuseIPDB：**
- 域名 IP 的滥用报告
- 滥用置信度评分
- 报告分类

**GreyNoise：**
- 域名 IP 是否被发现扫描互联网
- 分类：良性 / 恶意 / 未知

**PhishTank：**
- 是否被列为钓鱼站点
- 历史钓鱼报告

---

## 阶段 8：部署研究编队

**并行启动 8 名研究代理，按来源定向提示词：**

```typescript
// WHOIS 与注册情报 — DomainTools, ViewDNS
Task({ subagent_type: "PerplexityResearcher", prompt: "通过 DomainTools 与 ViewDNS 研究 [domain] 的注册信息，获取 WHOIS 历史、注册人变更、注册人邮箱的反向 WHOIS，以及域名服务器历史，报告注册时间线与所有权模式。" })

// DNS 与子域枚举 — SecurityTrails, DNSDumpster, crt.sh
Task({ subagent_type: "ClaudeResearcher", prompt: "通过 SecurityTrails（当前+历史 DNS）、DNSDumpster（被动侦察）与 crt.sh（证书透明度）枚举 [domain] 的子域与 DNS 记录，汇总所有子域及其 IP 解析。" })

// 技术指纹 — BuiltWith, Wappalyzer, Netcraft
Task({ subagent_type: "GeminiResearcher", prompt: "使用 BuiltWith、Wappalyzer 与 Netcraft 画像 [domain] 的技术栈，识别 Web 框架、CMS、CDN、分析工具、托管商与第三方集成。" })

// 基础设施扫描 — Shodan, Censys
Task({ subagent_type: "PerplexityResearcher", prompt: "在 Shodan 与 Censys 中检索 [domain] 及其 IP，报告开放端口、服务、SSL 证书、服务器版本与已知漏洞。" })

// 信誉评估 — VirusTotal, URLScan.io, PhishTank
Task({ subagent_type: "GrokResearcher", prompt: "通过 VirusTotal（扫描结果与社区评分）、URLScan.io（实时分析与重定向）与 PhishTank（钓鱼报告）评估 [domain] 信誉，报告检出比例与威胁分类。" })

// 滥用与威胁情报 — AbuseIPDB, GreyNoise, Pulsedive
Task({ subagent_type: "GrokResearcher", prompt: "检查 [domain] IP 在 AbuseIPDB（滥用报告与置信度）、GreyNoise（扫描分类）与 Pulsedive（威胁聚合）中的记录，报告滥用历史与威胁指标。" })

// 泄露暴露 — HIBP, Intelligence X
Task({ subagent_type: "GeminiResearcher", prompt: "在 HIBP 与 Intelligence X 中检索与 [domain] 相关的邮箱泄露，检查凭证泄露、paste 暴露与暗网提及。" })

// 关联域名发现 — 反向 WHOIS、证书 SAN、IP 邻居
Task({ subagent_type: "ClaudeResearcher", prompt: "通过反向 WHOIS（同注册人）、证书 SAN 条目、共享 IP 与共享域名服务器发现与 [domain] 关联的域名，绘制该实体完整域名足迹。" })
```

---

## 阶段 9：综合

**基础设施地图：**
- 域名 → 子域 → IP → 托管商
- 各子域技术栈
- 证书关联关系
- DNS 依赖链

**风险评估：**
- 域名年龄与稳定性
- 安全姿态（SPF、DMARC、DKIM、HTTPS、HSTS）
- 暴露服务与漏洞
- 滥用/威胁情报标记
- 泄露暴露

**关联域名：**
- 同注册人
- 同 IP/托管
- 证书 SAN 关联
- 相同域名服务器

**报告结构：**
1. 域名画像（注册信息、年龄、注册人）
2. DNS 基础设施（记录、DNS 服务器、邮件）
3. 子域地图（状态与用途）
4. 技术栈
5. 证书分析
6. 信誉与威胁情报
7. 关联域名
8. 风险评估
9. 建议

---

## 清单

- [ ] 授权已核验
- [ ] WHOIS/注册分析完成
- [ ] DNS 全量枚举完成
- [ ] 子域发现（5 种技术）完成
- [ ] 技术指纹识别完成
- [ ] 证书透明度分析完成
- [ ] 信誉/威胁情报检查完成
- [ ] 关联域名映射完成
- [ ] 风险评分已赋值
- [ ] 报告已起草

---

**参考：**完整来源目录见 `SOURCES.JSON`。
