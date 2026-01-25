---
名称: 优化wunder
描述: 用于调试和优化 wunder 智能体系统（上下文压缩、轮次统计、SSE 事件、payload 诊断、稳定性验证）。当需要复现问题、评估压缩行为、确认 token 统计或生成调试报告时使用。
---

# 优化wunder（调试指南）

## 快速目标
- 复现并记录上下文压缩触发点与效果
- 获取完整请求体以定位模型行为
- 验证技能调用的正确性与稳定性
- 输出可执行的优化建议与验证步骤

## 调试流程（通用）
1) 明确目标
   - 是看压缩行为、轮次统计、工具调用，还是技能熟练度
2) 确认配置与限制
   - max_context 与 max_output 预留是否合理
   - 必须启用 `debug_payload: true` 获取完整请求体（必要时再启用 `observability.log_level=DEBUG`）
   - `monitor_payload_max_chars` 为 0 以避免截断
3) 设计用例
   - 一次只验证一个关键点，避免混合太多目标
   - 控制单轮输出长度，避免无效极端填充
   - 若终端乱码，优先用 ASCII 文本做对照验证
4) 发起请求并收集 SSE
   - 用 `/wunder` 入口（支持 `debug_payload`）
   - 关注事件：`context_usage`、`compaction`、`llm_request`、`round_usage`、`final`
5) 判读结果
   - 压缩触发是否符合阈值与预期轮次
   - CHECK 轮次的 k=v 参数是否完整保留
   - `payload_omitted` 不应出现（否则说明未记录完整请求体）
   - user_round/model_round 与统计字段是否一致
   - token 统计记录的是“上下文占用量”，不是总消耗量
6) 输出优化建议
   - 明确问题 → 影响 → 修改建议 → 验证步骤
7) 如需改动
   - 更新 `docs/功能迭代.md`（脚本写入）
   - 若结构变化，同步 `docs/设计方案.md` / `docs/API文档.md` / `docs/系统介绍.md`

## 技能调试专项（高频问题）
- 必须先读 SKILL.md，再调用工具；未读即执行视为失败。
- 技能脚本只在工作目录执行：先将脚本复制到工作目录（如 `./scripts/`），再用相对路径运行。
- 若用户明确“不使用问询面板/直接执行”，不得调用问询面板。
- 工具执行后必须用 `列出文件` 或读取文件确认产物真实存在。
- 技能创建器避免预创建双重目录：`--path` 应为父目录，脚本会自动追加 `skill-name` 目录。

## 常见故障排查
- `payload_omitted: true`：未开启 `debug_payload` 或 payload 被截断。
- 中文变成 `????`：检查请求链路与日志展示编码；用 ASCII 对照用例定位问题源头。
- `不允许使用绝对路径`：改用相对路径或确保路径位于允许范围。
- `No such file or directory`：脚本未复制到工作目录或路径写错。
- `max_rounds` 终止：拆分任务或减少无关步骤。

## 关键注意事项
- `/wunder/chat` 默认不会传 `debug_payload`，完整 payload 请走 `/wunder`。
- 调试时必须设置 `debug_payload: true`，否则无法拿到完整请求体。
- `debug_payload` 或 DEBUG 会记录完整上下文，注意敏感信息。
- Windows 控制台乱码时设置 `PYTHONIOENCODING=utf-8`，或使用 ASCII 测试文本。
- 避免在 `frontend/` 目录做大范围搜索。

## 实战示例

### 示例 A：验证公文写作技能执行链路
**目标**：确认“读技能 → 写 draft → 执行脚本 → 产物确认”完整闭环。  
**请求要点**：明确要求“不要问询面板、在工作目录执行脚本、列出文件确认”。  
**验收点**：
- 先读 SKILL.md
- `draft.md` 被写入
- 运行脚本生成 `notice.docx`
- `列出文件` 能看到 `notice.docx`

### 示例 B：验证 PPTX 生成稳定性
**目标**：确认依赖检查、生成脚本、产物存在与内容正确。  
**验收点**：
- 先读 SKILL.md
- 依赖检查 `node -e "require('pptxgenjs')"` 执行过
- `build.js` 生成并执行
- `report.pptx` 在工作目录可见

### 示例 C：验证技能创建器最短流程
**目标**：确保 `init → edit → package` 一次成功。  
**验收点**：
- `init_skill.py <skill-name> --path <parent>` 正确执行
- `SKILL.md` frontmatter 完整
- 打包生成 `.skill` 到 `dist/`

## 常用定位文件
- 压缩流程：`src/orchestrator/memory.rs`
- 摘要注入：`src/services/history.rs`
- 提示词：`prompts/compact_prompt.txt`、`prompts/system.txt`
- 事件与统计：`src/orchestrator/execute.rs`

## SSE 简易脚本（示例）
```python
import json
import requests

url = "http://127.0.0.1:18000/wunder"
headers = {"X-API-Key": "YOUR_KEY", "Accept": "text/event-stream"}

payload = {
  "user_id": "debug-user",
  "session_id": "debug-session",
  "question": "ping",
  "stream": True,
  "skip_tool_calls": True,
  "tool_names": ["__no_tools__"],
  "debug_payload": True
}

def sse_events(resp):
    event, data = None, []
    for raw in resp.iter_lines(decode_unicode=True):
        if raw is None:
            continue
        line = raw.strip("\r")
        if line == "":
            if event or data:
                yield event, "\n".join(data)
            event, data = None, []
            continue
        if line.startswith("event:"):
            event = line[len("event:"):].strip()
        elif line.startswith("data:"):
            data.append(line[len("data:"):].strip())

resp = requests.post(url, headers=headers, json=payload, stream=True, timeout=60)
resp.raise_for_status()

for ev, data in sse_events(resp):
    if ev in {"context_usage", "compaction", "llm_request", "round_usage", "final"}:
        print(ev, json.loads(data).get("data"))
```
