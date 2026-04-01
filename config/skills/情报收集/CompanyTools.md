# 公司 OSINT 工具参考

## 商业情报数据库

### 初创与科技情报

**Crunchbase** (crunchbase.com)
- **用途：** 初创与科技公司数据库
- **可获得数据：** 融资、投资方、并购、管理层
- **费用：** 免费（有限），Pro（$29/月），Enterprise（定制）
- **覆盖：** 全球 300 万+ 公司

**PitchBook** (pitchbook.com)
- **用途：** PE/VC 交易数据库
- **可获得数据：** 估值、并购、基金表现
- **费用：** 机构约 $10,000-30,000/年
- **覆盖：** 300 万+ 公司，1 万+ 投资方

**AngelList** (angel.co)
- **用途：** 初创平台
- **可获得数据：** 初创公司、融资、投资者
- **费用：** 免费

### 通用商业情报

**ZoomInfo** (zoominfo.com)
- **用途：** B2B 情报
- **可获得数据：** 公司、联系人、组织架构、技术画像
- **费用：** 企业版 $15,000+/年
- **覆盖：** 1 亿+ 联系人，1,400 万+ 公司

**Bloomberg Terminal** (bloomberg.com/professional)
- **用途：** 金融数据平台
- **费用：** 约 $20,000-25,000/年
- **覆盖：** 全球金融市场数据

### 免费企业注册库

**OpenCorporates** (opencorporates.com)
- **用途：** 公司数据聚合
- **可获得数据：** 注册信息、管理层、状态
- **费用：** 免费（基础），API（付费）
- **覆盖：** 2 亿+ 公司，130+ 法域

**SEC EDGAR** (sec.gov/edgar)
- **用途：** 美国上市公司披露
- **可获得数据：** 10-K、10-Q、8-K、Proxy
- **费用：** 免费
- **API：** 提供（免费）

## 域名与 DNS 情报

**DomainTools** (domaintools.com)
- **用途：** 域名研究
- **可获得数据：** WHOIS 历史、DNS、截图
- **费用：** $99/月起
- **功能：** 反向 WHOIS、监控

**SecurityTrails** (securitytrails.com)
- **用途：** DNS 情报
- **可获得数据：** 历史 DNS、子域、证书
- **费用：** 免费（有限），Explorer（$49/月）
- **覆盖：** 40 亿+ DNS 记录

**crt.sh** (crt.sh)
- **用途：** 证书透明度查询
- **场景：** 子域发现
- **费用：** 免费
- **搜索：** `%.company.com`

**DNSDumpster** (dnsdumpster.com)
- **用途：** DNS 侦察
- **可获得数据：** 子域、MX、网络拓扑
- **费用：** 免费

## 网络与基础设施

**Shodan** (shodan.io)
- **用途：** 互联网设备搜索
- **可获得数据：** 端口、服务、漏洞
- **费用：** 免费（有限），Membership（$59/月）
- **搜索过滤：** org:, net:, ssl:, port:

**Censys** (censys.io)
- **用途：** 互联网扫描
- **可获得数据：** 主机、证书、服务
- **费用：** 免费（有限），Teams（$99/月）

**IPinfo** (ipinfo.io)
- **用途：** IP 地理定位
- **可获得数据：** 位置、ASN、公司
- **费用：** 免费（5 万/月），Basic（$49/月）

## 技术画像

**BuiltWith** (builtwith.com)
- **用途：** 技术识别
- **可获得数据：** 技术栈、托管、分析
- **费用：** 免费（有限），Pro（$295/月）
- **覆盖：** 6.7 亿+ 网站

**Wappalyzer** (wappalyzer.com)
- **用途：** 技术识别
- **可获得数据：** CMS、框架、分析工具
- **费用：** 免费（插件），Credits（$49/月）
- **技术库：** 3,000+ 追踪项

## 员工与人员

**LinkedIn** (linkedin.com)
- **用途：** 员工画像
- **可获得数据：** 团队、组织结构、招聘
- **费用：** 免费（有限），Sales Navigator（$80/月）

**Glassdoor** (glassdoor.com)
- **用途：** 员工评价
- **可获得数据：** 评价、薪资、文化
- **费用：** 免费

**Hunter.io** (hunter.io)
- **用途：** 邮箱发现
- **费用：** 免费（50/月），Starter（$49/月）

## 新闻与公关

**Google News** (news.google.com)
- **用途：** 新闻聚合
- **费用：** 免费

**PR Newswire** (prnewswire.com)
- **用途：** 新闻稿档案
- **费用：** 免费（搜索）

## 竞品情报

**SimilarWeb** (similarweb.com)
- **用途：** 网站流量分析
- **可获得数据：** 流量、来源、竞品
- **费用：** 免费（有限），Pro（$167/月）

**SEMrush** (semrush.com)
- **用途：** SEO 情报
- **可获得数据：** 关键词、反链、流量
- **费用：** Pro（$120/月）

## 历史与归档

**Wayback Machine** (archive.org)
- **用途：** 历史快照
- **覆盖：** 6700 亿+ 网页
- **费用：** 免费

---

## 工具选择矩阵

### 基础公司研究：
- 起步：官网、LinkedIn、Crunchbase、OpenCorporates
- 免费：SEC EDGAR（上市公司）、Google 搜索
- 技术：BuiltWith、Wappalyzer

### 技术基础设施：
- 域名：DomainTools、DNSDumpster、SecurityTrails
- 网络：Shodan、Censys、IPinfo
- 技术栈：BuiltWith、Wappalyzer

### 人员情报：
- 员工：LinkedIn、Hunter.io
- 文化：Glassdoor、Indeed
- 联系：Hunter.io、公司目录

### 深度研究：
- 商业：ZoomInfo、PitchBook（有预算）
- 技术：Shodan、SecurityTrails
- 历史：Wayback Machine
- 竞品：SimilarWeb、SEMrush

---

**提醒：** 组合多种工具，避免单一来源。务必交叉验证。
