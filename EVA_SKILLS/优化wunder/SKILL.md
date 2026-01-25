---
名称: 优化wunder
描述: 用于调试和优化 wunder 智能体系统（上下文压缩、轮次统计、SSE 事件、payload 诊断、稳定性验证）。当需要复现问题、评估压缩行为、确认 token 统计或生成调试报告时使用。
---

# 优化wunder（调试指南）

## 快速目标
- 复现并记录上下文压缩触发点与效果
- 获取完整请求体以定位模型行为
- 输出可执行的优化建议与验证步骤

## 调试流程
1) 确认配置与限制
   - max_context 当前值与 max_output 预留
   - 必须启用 `debug_payload: true` 获取完整请求体（必要时再启用 `observability.log_level=DEBUG`）
   - `monitor_payload_max_chars` 为 0 以避免截断
2) 选择测试场景
   - 使用真实业务文本，混合规则/参数/k=v/约束
   - 控制单轮输出长度，避免无效极端填充
   - 若终端乱码，优先用 ASCII 文本或设置编码
3) 发起请求并收集 SSE
   - 用 `/wunder` 入口（支持 `debug_payload`）
   - 关注事件：`context_usage`、`compaction`、`llm_request`、`round_usage`、`final`
4) 判读结果
   - 压缩触发是否符合阈值与预期轮次
   - CHECK 轮次的 k=v 参数是否完整保留
   - `payload_omitted` 不应出现（否则说明未记录完整请求体）
   - user_round/model_round 与统计字段是否一致
5) 输出优化建议
   - 明确问题 → 影响 → 修改建议 → 验证步骤
6) 如需改动
   - 更新 `docs/功能迭代.md`（脚本写入）
   - 若结构变化，同步 `docs/设计方案.md` / `docs/API文档.md` / `docs/系统介绍.md`

## 关键注意事项
- `/wunder/chat` 默认不会传 `debug_payload`，完整 payload 请走 `/wunder`。
- 调试时必须设置 `debug_payload: true`，否则无法拿到完整请求体。
- `debug_payload` 或 DEBUG 会记录完整上下文，注意敏感信息。
- Windows 控制台乱码时设置 `PYTHONIOENCODING=utf-8`，或使用 ASCII 测试文本。
- 避免在 `frontend/` 目录做大范围搜索。

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
