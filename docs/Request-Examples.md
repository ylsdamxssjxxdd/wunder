# Request Samples

This document provides Python request samples for the `/wunder` API, including the simplest call and a full streaming flow.

## 0. Dependencies & assumptions

- Python dependency: `pip install requests`
- Example base URL: `http://127.0.0.1:18000/wunder` (adjust for your deployment)

## 1. Minimal /wunder request (non-stream)

```python
import requests

# /wunder entry, default local dev port
BASE_URL = "http://127.0.0.1:18000/wunder"

# Minimal payload: user_id and question are required
payload = {
    "user_id": "u001",
    "question": "Hello",
    "stream": False,  # disable streaming, return full answer
}

# Send request and check HTTP status
resp = requests.post(BASE_URL, json=payload, timeout=60)
resp.raise_for_status()

# Parse JSON response with session_id and answer
data = resp.json()
print(data["session_id"])
print(data["answer"])
```

## 2. Full agent flow (fetch tools + stream response)

```python
import json
import requests

# Shared base to avoid hardcoding
BASE_HOST = "http://127.0.0.1:18000"
TOOLS_URL = f"{BASE_HOST}/wunder/tools"
WUNDER_URL = f"{BASE_HOST}/wunder"

# 1) Fetch available tools from the server
tools_resp = requests.get(TOOLS_URL, timeout=30)
tools_resp.raise_for_status()
tools_payload = tools_resp.json()

# 2) Extract tool names from each list and deduplicate

def _append_tools(target, items):
    for item in items:
        if not isinstance(item, dict):
            continue
        name = str(item.get("name", "")).strip()
        if name:
            target.append(name)

tool_names_raw = []
_append_tools(tool_names_raw, tools_payload.get("builtin_tools", []))
_append_tools(tool_names_raw, tools_payload.get("mcp_tools", []))
_append_tools(tool_names_raw, tools_payload.get("skills", []))
_append_tools(tool_names_raw, tools_payload.get("knowledge_tools", []))

seen = set()
tool_names = []
for name in tool_names_raw:
    if name in seen:
        continue
    seen.add(name)
    tool_names.append(name)

# 3) Build /wunder request with all tool names
payload = {
    "user_id": "u001",
    "question": "Please complete the task using available tools and explain the process.",
    "tool_names": tool_names,
    "stream": True,
}

# 4) Receive SSE stream and parse events
with requests.post(WUNDER_URL, json=payload, stream=True, timeout=120) as resp:
    resp.raise_for_status()
    current_event = None
    for line in resp.iter_lines(decode_unicode=True):
        if not line:
            continue
        if line.startswith("event:"):
            current_event = line.split(":", 1)[1].strip()
            continue
        if not line.startswith("data:"):
            continue
        # data line is JSON, containing type/session_id/data/timestamp
        event = json.loads(line.split(":", 1)[1].strip())
        event_type = current_event or event.get("type")
        event_data = event.get("data", {})
        print(event_type, event_data)
        if event_type == "final":
            break
```
