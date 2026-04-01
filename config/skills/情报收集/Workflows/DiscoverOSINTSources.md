# 发现新 OSINT 来源工作流

## 语音通知

```bash
curl -s -X POST http://localhost:8888/notify \
  -H "Content-Type: application/json" \
  -d '{"message": "正在运行 DiscoverOSINTSources 工作流以发现新 OSINT 来源并更新清单"}' \
  > /dev/null 2>&1 &
```

正在运行 **DiscoverOSINTSources** 工作流（技能：**情报收集**）以发现并整理新的 OSINT 来源...

**目的：**通过并行研究代理发现、评估并集成新的 OSINT 来源到 SOURCES.JSON 与 SOURCES.md 清单中。

---

## 阶段 1：加载当前来源清单

**读取现有来源：**
- 读取 `~/.claude/skills/Investigation/情报收集/SOURCES.JSON` 获取当前来源数量与分类
- 记录 `last_updated` 日期以判断陈旧程度
- 建立现有来源 URL 列表用于去重

**识别缺口：**
- 检查各分类中 `"status": "stale"` 或 `"status": "unknown"` 的来源
- 记录来源数量少于 10 的分类（潜在缺口）
- 记录本次发现的时间戳

---

## 阶段 2：部署研究编队

**并行启动 4-6 名研究代理，覆盖以下搜索域：**

1. **GitHub 发现代理**（ClaudeResearcher）
   - 搜索 GitHub 主题：osint、osint-tools、reconnaissance、threat-intelligence、people-search
   - 查找在 last_updated 之后新增或重大更新的仓库
   - 查看知名 OSINT 组织的加星仓库（cipher387、The-Osint-Toolbox、bellingcat、projectdiscovery）
   - 筛选条件：>50 stars 或最近 90 天内创建且内容有价值

2. **Web 目录代理**（PerplexityResearcher）
   - 查看 Week in OSINT（sector035.nl）近期推荐工具
   - 搜索新的 start.me OSINT 页面
   - 检查 Bellingcat toolkit 最新增补
   - 搜索 “new OSINT tool 2026”等关键词
   - 关注 OSINT subreddit 的热门工具/资源

3. **威胁情报代理**（GeminiResearcher）
   - 搜索新的威胁情报平台与情报订阅
   - 查找新漏洞数据库
   - 查找新的 IP/域名信誉服务
   - 关注 CISA、NCSC、ENISA 的新资源
   - 搜索新的恶意软件沙箱

4. **人员/商业代理**（GrokResearcher）
   - 搜索新的人物搜索引擎或用户名工具
   - 查找新的商业情报平台
   - 查找新的企业注册 API
   - 搜索新的社媒 OSINT 工具（重点关注新平台）
   - 查找新的邮箱/电话调查工具

5. **培训/社区代理**（PerplexityResearcher）
   - 搜索新的 OSINT 课程、CTF 或培训平台
   - 查找新的 OSINT 播客或通讯
   - 查找新的 OSINT 会议或社区活动
   - 关注新近崭露头角的 OSINT 从业者

6. **专项代理**（ClaudeResearcher）
   - 搜索新的地理定位/GEOINT 工具
   - 查找新的加密货币/区块链 OSINT 工具
   - 查找新的暗网监测工具
   - 搜索新的 AI 驱动 OSINT 工具
   - 查找区域性（非英语）OSINT 资源

---

## 阶段 3：评估与去重

**对每个新发现的来源：**

1. **去重检查：**
   - 与现有 SOURCES.JSON 条目对比 URL
   - 检查是否为重命名/改版的旧来源
   - 跳过完全重复项

2. **质量评估：**
   - 是否活跃维护？（12 个月内更新）
   - 是否提供现有来源未覆盖的独特价值？
   - 是否来自可信作者/组织？
   - 是否免费或有可用的免费层？

3. **分类：**
   - 指定主类与子类
   - 标注 OSINT 域：people、company、entity、threat
   - 标注费用层级：free、freemium、paid
   - 标注状态：active、stale、unknown

**质量门槛：**仅满足以下全部条件的来源通过：
- [ ] 非现有来源重复
- [ ] 活跃维护或为权威参考
- [ ] 提供独特调查价值
- [ ] 来自可信来源

---

## 阶段 4：更新来源文件

**更新 SOURCES.JSON：**
- 将新来源加入对应分类数组
- 更新 `last_updated` 为当天日期
- 增加 `total_sources` 数量
- 更新状态变化的来源（active → stale 等）

**更新 SOURCES.md：**
- 在对应章节表格中添加新条目
- 维持按字母或逻辑排序
- 更新底部统计表
- 保持格式与现有条目一致

**生成发现报告：**
- 列出新增来源与分类
- 列出状态变化的来源
- 指出仍较薄弱的分类
- 给出后续补查建议

---

## 阶段 5：校验与报告

**校验文件完整性：**
- 读取 SOURCES.JSON 并验证 JSON 有效
- 读取 SOURCES.md 并检查 Markdown 格式
- 确认新来源数量与预期一致
- 验证未误删既有来源

**汇报结果：**
```
## OSINT 来源发现报告

**运行日期：** [date]
**原来源数量：** [N]
**新增来源：** [N]
**更新后数量：** [N+new]
**状态变化来源：** [N]

### 按分类新增来源
[列出每个新增来源：名称、URL、分类]

### 缺口分析
[仍低于 10 个来源或覆盖陈旧的分类]

### 后续建议
[下次建议执行的具体搜索]
```

---

## 调度建议

此工作流建议：
- **每月**例行运行
- **重大 OSINT 会议后**运行（SANS OSINT Summit、DEFCON Recon Village、OSMOSIS）
- **出现新 OSINT 领域**时运行（如新社交平台、新威胁类别）
- **按需**运行（针对特定领域建设能力）
