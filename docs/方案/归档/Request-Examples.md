# Request Samples

This document provides Python request samples for the `/wunder` API, including a minimal call and a full streaming flow.

## 0. Dependencies & assumptions

- Python dependency: `pip install requests`
- Example base URL: `http://127.0.0.1:18000` (adjust for your deployment)
- API key: `X-API-Key` header is required for `/wunder` and most admin endpoints

## 1. Minimal /wunder request (non-stream)

```python
import requests

BASE_HOST = "http://127.0.0.1:18000"
BASE_URL = f"{BASE_HOST}/wunder"
API_KEY = "YOUR_KEY"

payload = {
    "user_id": "u001",
    "question": "Hello",
    "stream": False,
}

headers = {
    "X-API-Key": API_KEY,
    "Content-Type": "application/json",
}

resp = requests.post(BASE_URL, json=payload, headers=headers, timeout=60)
resp.raise_for_status()

data = resp.json()
print(data["session_id"])
print(data["answer"])
```

## 2. Full agent flow (fetch tools + stream response)

```python
import json
import requests

BASE_HOST = "http://127.0.0.1:18000"
API_KEY = "YOUR_KEY"
TOOLS_URL = f"{BASE_HOST}/wunder/tools"
WUNDER_URL = f"{BASE_HOST}/wunder"

headers = {
    "X-API-Key": API_KEY,
    "Content-Type": "application/json",
    "Accept": "text/event-stream",
}

# 1) Fetch available tools (use ?user_id=... to include user tools)
tools_resp = requests.get(f"{TOOLS_URL}?user_id=u001", headers={"X-API-Key": API_KEY}, timeout=30)
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
_append_tools(tool_names_raw, tools_payload.get("user_tools", []))
_append_tools(tool_names_raw, tools_payload.get("shared_tools", []))

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
    "debug_payload": True,
}

# 4) Receive SSE stream and parse events
with requests.post(WUNDER_URL, json=payload, headers=headers, stream=True, timeout=120) as resp:
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
        event = json.loads(line.split(":", 1)[1].strip())
        event_type = current_event or event.get("type")
        event_data = event.get("data", {})
        print(event_type, event_data)
        if event_type == "final":
            break
```

Notes:
- `debug_payload` is supported only on `/wunder`; `/wunder/chat` omits full request bodies.
- The stream emits multiple event types with the same event semantics over WS/SSE; see `docs/方案/API-Documentation.md` for details.
