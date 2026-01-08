import httpx

from app.llm.openai_compatible import probe_openai_context_window


async def test_probe_context_window_mindie_v1_config():
    async def handler(request: httpx.Request) -> httpx.Response:
        path = request.url.path
        if path in ("/v1/models/demo", "/v1/models", "/props", "/v2/models/demo/config"):
            return httpx.Response(404, json={"message": "not found"})
        if path == "/v1/config":
            return httpx.Response(200, json={"modelName": "demo", "maxSeqLen": 24576})
        return httpx.Response(404, json={"message": "unexpected", "path": path})

    async with httpx.AsyncClient(transport=httpx.MockTransport(handler)) as client:
        value = await probe_openai_context_window(
            base_url="http://mindie.test",
            api_key="",
            model="demo",
            timeout_s=5,
            client=client,
        )

    assert value == 24576


async def test_probe_context_window_mindie_v2_model_config():
    async def handler(request: httpx.Request) -> httpx.Response:
        path = request.url.path
        if path == "/v2/models/demo/config":
            return httpx.Response(200, json={"model_name": "demo", "max_seq_len": 28672})
        if path in ("/v1/models/demo", "/v1/models", "/props", "/v1/config"):
            return httpx.Response(404, json={"message": "not found"})
        return httpx.Response(404, json={"message": "unexpected", "path": path})

    async with httpx.AsyncClient(transport=httpx.MockTransport(handler)) as client:
        value = await probe_openai_context_window(
            base_url="http://mindie.test/v1",
            api_key="",
            model="demo",
            timeout_s=5,
            client=client,
        )

    assert value == 28672

