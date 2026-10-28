import asyncio
import json
from aiohttp import web

active = 0
peak = 0
calls = 0

async def health(request):
    return web.json_response(dict(active=active, peak=peak, calls=calls))

async def completion(request):
    global active, peak, calls
    payload = await request.json()
    active += 1
    calls += 1
    peak = max(peak, active)
    try:
        if not payload.get('stream'):
            await asyncio.sleep(.512)
            return web.json_response(dict(id='fixture-completion', choices=[dict(index=0,
                message=dict(role='assistant', content='fixture-complete'), finish_reason='stop')],
                usage=dict(prompt_tokens=32, completion_tokens=256, total_tokens=288)))
        response = web.StreamResponse(headers={'Content-Type': 'text/event-stream'})
        await response.prepare(request)
        for index in range(64):
            await asyncio.sleep(.008)
            chunk = dict(id='fixture-completion', choices=[dict(index=0,
                delta=dict(content='fixture-{:04d}-片段\n'.format(index)), finish_reason=None)])
            await response.write(('data: ' + json.dumps(chunk, ensure_ascii=False) + '\n\n').encode())
        await response.write(('data: ' + json.dumps(dict(id='fixture-completion', choices=[dict(
            index=0, delta={}, finish_reason='stop')], usage=dict(prompt_tokens=32,
            completion_tokens=256, total_tokens=288))) + '\n\ndata: [DONE]\n\n').encode())
        await response.write_eof()
        return response
    finally:
        active -= 1

app = web.Application(client_max_size=8*1024*1024)
app.add_routes([web.get('/health', health), web.post('/v1/chat/completions', completion)])
web.run_app(app, host='0.0.0.0', port=18090, access_log=None, print=None)

