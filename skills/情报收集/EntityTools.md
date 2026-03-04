# 实体 OSINT 工具参考

## 域名与 DNS 工具

### WHOIS 服务

**DomainTools** (domaintools.com)
- **用途：** 高级域名情报
- **可获得数据：** WHOIS 历史、DNS、IP 历史、风险评分
- **费用：** $99/月起
- **功能：** 反向 WHOIS、监控、API

**ViewDNS** (viewdns.info)
- **用途：** DNS 工具包
- **可获得数据：** DNS 记录、IP 历史、反向查询
- **费用：** 免费（有限），API（$10-50/月）

### DNS 侦察

**DNSDumpster** (dnsdumpster.com)
- **用途：** DNS 侦察
- **费用：** 免费，无需认证
- **功能：** 可视化网络图、HTTP/HTTPS 检测

**SecurityTrails** (securitytrails.com)
- **用途：** DNS 情报
- **费用：** 免费（50/月），Explorer（$49/月）
- **覆盖：** 45 亿 DNS 记录

**Amass** (github.com/OWASP/Amass)
- **用途：** 高级子域枚举
- **费用：** 免费、开源（OWASP）
- **功能：** 主动/被动侦察，55+ 来源

### 证书情报

**crt.sh** (crt.sh)
- **用途：** 证书透明度查询
- **费用：** 免费
- **搜索：** `%.example.com`

**Censys** (censys.io)
- **用途：** 证书资产清单
- **费用：** 免费（250/月），Teams（$99/月）

## IP 与网络工具

### 地理定位与归属

**IPinfo** (ipinfo.io)
- **用途：** IP 数据与洞察
- **费用：** 免费（5 万/月），Basic（$49/月）
- **CLI：** `npm install -g node-ipinfo`

**MaxMind GeoIP2** (maxmind.com)
- **用途：** 地理定位数据库
- **费用：** 免费（GeoLite2），付费（$30-700/月）

### ASN 与 BGP

**Hurricane Electric BGP** (bgp.he.net)
- **用途：** BGP 路由情报
- **费用：** 免费
- **功能：** ASN 查询、前缀信息、对等连接

**RIPE Stat** (stat.ripe.net)
- **用途：** 互联网测量
- **费用：** 免费
- **API：** REST + 小组件

### 互联网扫描

**Shodan** (shodan.io)
- **用途：** 设备搜索引擎
- **费用：** 免费（有限），Membership（$59/月）
- **搜索：** ip:, net:, org:, ssl:, port:, vuln:
- **CLI：** 可用

**Censys** (censys.io)
- **用途：** 互联网扫描
- **费用：** 免费（250/月），Teams（$99/月）

**BinaryEdge** (binaryedge.io)
- **用途：** 网络安全数据
- **费用：** 免费（250/月），Pro（$10/月）

### IP 信誉

**AbuseIPDB** (abuseipdb.com)
- **用途：** IP 滥用数据库
- **费用：** 免费（1,000 次/天），API（分级）
- **评分：** 0-100% 置信度

**GreyNoise** (greynoise.io)
- **用途：** 扫描器分类
- **费用：** 社区版（免费），企业版（$500+/月）

## 威胁情报

### 恶意软件分析

**VirusTotal** (virustotal.com)
- **用途：** 多引擎扫描分析
- **费用：** 免费（有限），Premium（$180/月）
- **能力：** 文件（650MB）、URL、IP、哈希
- **API：** 免费版 4 次/分钟

**Hybrid Analysis** (hybrid-analysis.com)
- **用途：** 自动化恶意软件沙箱
- **费用：** 免费（公共），企业版（私有）
- **沙箱：** CrowdStrike Falcon

**Malware Bazaar** (bazaar.abuse.ch)
- **用途：** 恶意样本共享
- **费用：** 免费
- **数据库：** 300 万+ 样本

### 威胁平台

**AlienVault OTX** (otx.alienvault.com)
- **用途：** 威胁情报社区
- **费用：** 免费
- **功能：** Pulses、IoCs、对手画像

**MITRE ATT&CK** (attack.mitre.org)
- **用途：** 对手 TTP 知识库
- **费用：** 免费
- **覆盖：** 14 战术、193 技术、127 组织

**ThreatFox** (threatfox.abuse.ch)
- **用途：** IoC 共享
- **费用：** 免费
- **导出：** JSON、CSV、MISP

### URL 分析

**URLScan.io** (urlscan.io)
- **用途：** 网站扫描
- **费用：** 免费（公开），Pro（$150/年私有）
- **功能：** 截图、DOM、技术识别

**URLhaus** (urlhaus.abuse.ch)
- **用途：** 恶意 URL 数据库
- **费用：** 免费

## 历史与归档

**Wayback Machine** (archive.org)
- **用途：** 历史快照
- **覆盖：** 7350 亿+ 页面
- **API：** Wayback API、CDX API

## 自动化框架

**Maltego** (maltego.com)
- **用途：** 可视化关联分析
- **费用：** 社区版（免费），Classic（$999/年）
- **场景：** 关系映射

**SpiderFoot** (spiderfoot.net)
- **用途：** 自动化 OSINT
- **费用：** 免费（开源），HX（商业版）
- **模块：** 200+

**Recon-ng** (github.com/lanmaster53/recon-ng)
- **用途：** 侦察框架
- **费用：** 免费、开源
- **模块：** 90+

---

## 工具选择指南

### 域名情报：
- 快速：whois.com、DNSDumpster
- 全面：DomainTools、SecurityTrails
- 子域：crt.sh、Amass

### IP 情报：
- 地理：IPinfo、MaxMind
- ASN/BGP：Hurricane Electric、RIPE Stat
- 信誉：AbuseIPDB、AlienVault OTX
- 扫描：Shodan、Censys

### 威胁情报：
- 文件：VirusTotal、Hybrid Analysis、Malware Bazaar
- URL：URLScan.io、URLhaus
- IoC：ThreatFox、AlienVault OTX

### 自动化：
- 可视化：Maltego
- 全自动：SpiderFoot
- 模块化：Recon-ng

---

**提醒：** 多工具交叉验证，尊重限速与 ToS，仅对已授权目标使用。

---

## 攻击目标画像模式

### 企业 vs. 个人目标区分

**背景：** 攻击者会对目标进行指纹识别，以判断价值并选择合适的载荷。

**识别企业目标的指标：**
| 指标 | 方法 | 权重 |
|-----------|--------|--------|
| 域加入机器 | AD 枚举 | 高 |
| 企业邮箱域名 | 浏览器会话 | 高 |
| VPN/代理检测 | 网络配置 | 中 |
| 企业软件 | 进程列表 | 中 |
| 受管浏览器策略 | 注册表/配置 | 高 |

**对 OSINT 的意义：**
- 调查威胁行为体时关注其目标选择逻辑
- 检测域加入 = 企业间谍动机
- 仅针对个人 = 不同威胁模型（加密货币、网银）
- 混合目标 = 具备分层访问售卖的商品化恶意软件

**攻击者常用枚举命令：**
```powershell
# 域成员检查
(Get-WmiObject -Class Win32_ComputerSystem).PartOfDomain

# 完整 AD 域信息
[System.DirectoryServices.ActiveDirectory.Domain]::GetCurrentDomain()

# 检查企业软件
Get-WmiObject -Class Win32_Product | Where-Object {$_.Name -like "*endpoint*"}
```

**分析问题：**
1. 恶意软件是否在执行前检测域状态？
2. 企业与个人是否存在不同载荷路径？
3. C2 基础设施是否有分层定价/访问？

### 浏览器扩展攻击模式

**“崩溃修复”攻击链：**
1. 通过拼写近似的扩展伪装成合法插件（uBlock Origin 等）
2. 使用 Chrome Alarms API 延迟 60+ 分钟（规避）
3. 触发伪造浏览器崩溃
4. 社工诱导执行剪贴板载荷
5. 使用 LOLBIN（如 finger.exe）建立 C2

**OSINT 价值：**
- 追踪扩展发布者账号跨平台关联
- 绘制 C2 基础设施
- 查找同类技术的相似扩展
- 与威胁组织 TTP 关联

**参考：**见 `~/.claude/skills/Parser/Workflows/ExtractBrowserExtension.md` 的分析流程。
