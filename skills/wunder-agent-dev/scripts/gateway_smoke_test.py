import asyncio
import json
import os
import time

try:
    import aiohttp
    import websockets
except Exception as exc:
    raise SystemExit(
        "Missing dependencies. Install aiohttp + websockets. Error: %s" % exc
    )

BASE_URL = os.getenv("WUNDER_BASE_URL", "http://127.0.0.1:18000")
TOKEN = os.getenv("WUNDER_API_KEY", "ylsdamxssjxxdd")
NODE_ID = os.getenv("WUNDER_NODE_ID", "demo-node")
WS_URL = os.getenv("WUNDER_GATEWAY_WS", BASE_URL.replace("http", "ws", 1) + "/wunder/gateway/ws")

async def wait_status(session, headers, retries=30, delay=2):
    last_error = None
    for _ in range(retries):
        try:
            async with session.get(
                f"{BASE_URL}/wunder/admin/gateway/status", headers=headers
            ) as resp:
                if resp.status == 200:
                    return await resp.json()
                last_error = f"status={resp.status}"
        except Exception as exc:
            last_error = str(exc)
        await asyncio.sleep(delay)
    raise RuntimeError(f"gateway status not ready: {last_error}")

async def recv_json(ws, timeout_s):
    raw = await asyncio.wait_for(ws.recv(), timeout_s)
    return json.loads(raw)

async def main():
    headers = {"Authorization": f"Bearer {TOKEN}"}
    timeout = aiohttp.ClientTimeout(total=60)
    async with aiohttp.ClientSession(timeout=timeout) as session:
        status = await wait_status(session, headers)
        print("gateway_status:", status)

        async with session.post(
            f"{BASE_URL}/wunder/admin/gateway/node_tokens",
            json={"node_id": NODE_ID},
            headers=headers,
        ) as resp:
            token_payload = await resp.json()
        node_token = token_payload.get("data", {}).get("token")
        if not node_token:
            raise RuntimeError(f"failed to create node token: {token_payload}")
        print("node_token:", node_token)

        async with websockets.connect(
            WS_URL,
            subprotocols=["wunder-gateway"],
            max_size=512 * 1024,
            additional_headers=headers,
        ) as ws:
            connect_req = {
                "type": "req",
                "id": "connect-1",
                "method": "connect",
                "params": {
                    "role": "node",
                    "min_protocol": 1,
                    "max_protocol": 1,
                    "auth": {"token": TOKEN, "node_token": node_token},
                    "device": {"id": NODE_ID, "name": "demo-node"},
                    "client": {"id": "demo-node", "version": "sim-1.0", "platform": "python"},
                    "caps": ["ping"],
                    "commands": ["ping"],
                },
            }
            await ws.send(json.dumps(connect_req))

            hello_ok = None
            start = time.time()
            while time.time() - start < 15:
                msg = await recv_json(ws, 15)
                if msg.get("type") == "event" and msg.get("event") == "connect.challenge":
                    continue
                if msg.get("type") == "res" and msg.get("payload", {}).get("type") == "hello-ok":
                    hello_ok = msg
                    break
            if not hello_ok:
                raise RuntimeError("gateway hello-ok not received")
            print("node_connected:", hello_ok.get("payload", {}).get("connection_id"))

            async def wait_and_reply():
                while True:
                    msg = await recv_json(ws, 20)
                    if msg.get("type") == "req" and msg.get("method") == "node.invoke":
                        print("node_invoke_received:", msg.get("params"))
                        await ws.send(
                            json.dumps(
                                {
                                    "type": "res",
                                    "id": msg.get("id"),
                                    "ok": True,
                                    "payload": {"status": "ok", "echo": msg.get("params")},
                                }
                            )
                        )
                        return

            async def admin_invoke():
                payload = {
                    "node_id": NODE_ID,
                    "command": "ping",
                    "args": {"text": "hello"},
                    "timeout_s": 10,
                    "metadata": {"source": "gateway-smoke"},
                }
                async with session.post(
                    f"{BASE_URL}/wunder/admin/gateway/invoke",
                    json=payload,
                    headers=headers,
                ) as resp:
                    return await resp.json()

            reply_task = asyncio.create_task(wait_and_reply())
            invoke_task = asyncio.create_task(admin_invoke())
            await asyncio.gather(reply_task, invoke_task)
            print("invoke_response:", invoke_task.result())

if __name__ == "__main__":
    asyncio.run(main())
