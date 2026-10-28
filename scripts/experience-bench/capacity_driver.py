import argparse
import asyncio
from collections import Counter
import json
import math
from pathlib import Path
import time

import aiohttp

expected = ''.join('fixture-{:04d}-片段\n'.format(index) for index in range(64))

def summary(values):
    values = sorted(values)
    if not values:
        return dict(samples=0)
    return dict(samples=len(values), p50_ms=values[math.ceil(len(values)*.5)-1],
        p95_ms=values[math.ceil(len(values)*.95)-1], maximum_ms=values[-1])

async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--levels', default='100,500,2000')
    parser.add_argument('--online-seconds', type=int, default=35)
    parser.add_argument('--label', default='baseline')
    args = parser.parse_args()
    directory=args.fixture.resolve()
    fixture=json.loads((directory/'capacity-credentials.json').read_text(encoding='utf-8'))
    connector = aiohttp.TCPConnector(limit=2600, limit_per_host=2600, ttl_dns_cache=300)
    timeout = aiohttp.ClientTimeout(total=240, connect=60)
    result = dict(users=2000, fixture='unique-accounts-and-authenticated-websockets',
                  max_active_tasks=300, api_workers=8, excludes='login-password-hashing-and-real-inference',
                  stages=[])
    async with aiohttp.ClientSession(connector=connector, timeout=timeout, trust_env=False) as http:
        async with http.post(fixture['base_url']+'/wunder/auth/login',json=dict(
            username=fixture['admin']['username'],password=fixture['admin']['password'])) as response:
            assert response.status == 200
            admin = (await response.json())['data']['access_token']
        headers = dict(Authorization='Bearer '+admin)
        async def runtime_snapshot():
            async with http.get(fixture['base_url']+'/wunder/admin/runtime_metrics',
                                headers=headers) as response:
                assert response.status == 200
                return (await response.json())['runtime']
        stop = asyncio.Event()
        probes = []
        async def observer():
            while not stop.is_set():
                for kind, suffix in [('health','/wunder/i18n'),('monitor','/wunder/admin/monitor')]:
                    began=time.perf_counter()
                    try:
                        async with http.get(fixture['base_url']+suffix,headers=headers,
                                            timeout=aiohttp.ClientTimeout(total=45)) as response:
                            body=await response.read()
                            probes.append(dict(kind=kind,status=response.status,bytes=len(body),
                                ms=(time.perf_counter()-began)*1000))
                    except Exception as error:
                        probes.append(dict(kind=kind,error=type(error).__name__,ms=(time.perf_counter()-began)*1000))
                try:
                    await asyncio.wait_for(stop.wait(),timeout=1)
                except asyncio.TimeoutError:
                    pass
        observe=asyncio.create_task(observer())
        users=fixture['users']
        prepare_limit=asyncio.Semaphore(24)
        async def prepare(user):
            async with prepare_limit:
                async with http.post(fixture['base_url']+'/wunder/chat/sessions',
                    headers=dict(Authorization='Bearer '+user['token']),json=dict(title='fixture-capacity'),
                    timeout=aiohttp.ClientTimeout(total=60)) as response:
                    assert response.status==200, 'session_creation_status_'+str(response.status)
                    user['session_id']=(await response.json())['data']['id']
        print('Preparing 2000 independent sessions',flush=True)
        await asyncio.gather(*(prepare(user) for user in users))
        sockets=[None]*2000
        failures=Counter()
        connections=[]
        open_limit=asyncio.Semaphore(100)
        async def connect(index,user):
            async with open_limit:
                began=time.perf_counter()
                try:
                    socket=await http.ws_connect(fixture['base_url']+'/wunder/chat/ws',
                        headers=dict(Authorization='Bearer '+user['token']),protocols=['wunder'],
                        max_msg_size=2*1024*1024,autoping=True)
                    assert (await asyncio.wait_for(socket.receive_json(),30))['type']=='ready'
                    await socket.send_json(dict(type='connect',request_id='fixture-connect',payload=dict(protocol_version=1)))
                    assert (await asyncio.wait_for(socket.receive_json(),30))['type']=='ready'
                    sockets[index]=socket
                    connections.append((time.perf_counter()-began)*1000)
                except Exception as error:
                    failures['connect_'+type(error).__name__]+=1
        print('Connecting 2000 distinct authenticated users',flush=True)
        await asyncio.gather(*(connect(index,user) for index,user in enumerate(users)))
        async def ping(socket):
            if socket is None or socket.closed:
                failures['closed_socket']+=1
                return
            try:
                await socket.send_json(dict(type='ping'))
                response=await asyncio.wait_for(socket.receive_json(),45)
                assert response['type']=='pong'
            except Exception as error:
                failures['ping_'+type(error).__name__]+=1
        for _ in range(max(1,args.online_seconds//10)):
            await asyncio.sleep(10)
            await asyncio.gather(*(ping(socket) for socket in sockets))
        result['online']=dict(connected=sum(socket is not None and not socket.closed for socket in sockets),
            failures=dict(failures),connection_ms=summary(connections),hold_seconds=max(1,args.online_seconds//10)*10)
        print(json.dumps(dict(online=result['online'])),flush=True)
        for level in map(int,args.levels.split(',')):
            assert 1<=level<=2000
            if failures:
                break
            began=time.perf_counter()
            stage_probes=len(probes)
            stage_errors=Counter()
            error_samples=[]
            first=[]
            completed=[]
            queued_snapshots=0
            deferred_checks=[]
            queued=0
            async def run(index):
                nonlocal queued,queued_snapshots
                user=users[index]
                socket=sockets[index]
                key='fixture-capacity-{}-{}'.format(level,index)
                text=[]
                first_seen=False
                saw_queue=False
                snapshot_text=''
                try:
                    await socket.send_json(dict(type='start',request_id=key,session_id=user['session_id'],
                        payload=dict(session_id=user['session_id'],content=key,client_message_id=key,stream=True,view_seq=0)))
                    async with asyncio.timeout(240):
                        for _ in range(8192):
                            envelope=await socket.receive_json()
                            if envelope.get('type')=='error':
                                payload=envelope.get('payload',{})
                                stage_errors['protocol_'+str(payload.get('code','unknown'))]+=1
                                if len(error_samples)<8:
                                    error_samples.append(dict(code=payload.get('code'), status=payload.get('status')))
                                return
                            if envelope.get('request_id')!=key or envelope.get('type')!='event':
                                continue
                            event=envelope['payload']
                            name=event.get('event')
                            data=event.get('data',{})
                            if isinstance(data.get('data'),dict):
                                data=data['data']
                            if name=='queued' and not saw_queue:
                                queued+=1
                                saw_queue=True
                            if name=='error':
                                stage_errors['event_'+str(data.get('code',data.get('error_code','unknown')))]+=1
                                if len(error_samples)<8:
                                    error_samples.append(dict(event=name,code=data.get('code',data.get('error_code'))))
                                return
                            if name=='thread_item_tail' and data.get('field')=='content' and data.get('text'):
                                if not first_seen:
                                    first.append((time.perf_counter()-began)*1000)
                                    first_seen=True
                                if data['offset']!=len(''.join(text).encode('utf-16-le'))//2:
                                    stage_errors['tail_offset']+=1
                                text.append(data['text'])
                            if name=='thread_item_upsert':
                                item=data.get('item',data)
                                if item.get('kind')=='assistant' and isinstance(item.get('content'),str):
                                    text[:]=[item['content']]
                            if name=='thread_view_change':
                                for row in data.get('rows',[]):
                                    if row.get('client_message_id')==key:
                                        snapshot_text=row.get('assistant',{}).get('content','')
                                        if snapshot_text and not first_seen:
                                            first.append((time.perf_counter()-began)*1000)
                                            first_seen=True
                            if name=='final':
                                if saw_queue:
                                    if snapshot_text==expected:
                                        queued_snapshots+=1
                                    else:
                                        deferred_checks.append(index)
                                    completed.append((time.perf_counter()-began)*1000)
                                elif ''.join(text)!=expected:
                                    stage_errors['text_mismatch']+=1
                                    if len(error_samples)<8:
                                        error_samples.append(dict(final_text_mismatch=True,queued=saw_queue))
                                else:
                                    completed.append((time.perf_counter()-began)*1000)
                                return
                        stage_errors['frame_budget']+=1
                except Exception as error:
                    stage_errors[type(error).__name__]+=1
                    if len(error_samples)<8:
                        error_samples.append(dict(exception=type(error).__name__,close_code=socket.close_code))
            print('Starting simultaneous task burst: '+str(level),flush=True)
            async with http.get(fixture['model_url']+'/health') as response:
                model_before=await response.json()
            await asyncio.gather(*(run(index) for index in range(level)))
            async with http.get(fixture['model_url']+'/health') as response:
                model_after=await response.json()
            checks=Counter()
            async def verify(index):
                user=users[index]
                key='fixture-capacity-{}-{}'.format(level,index)
                async with prepare_limit:
                    try:
                        async with http.get(fixture['base_url']+'/wunder/chat/sessions/'+user['session_id']+'/view/bootstrap?kind=chat',
                            headers=dict(Authorization='Bearer '+user['token'])) as response:
                            assert response.status==200
                            payload=await response.json()
                            page=payload['data']['page']
                            rows=page.get('chat',[])
                            row=next(row for row in rows if row.get('client_message_id')==key)
                            assert row.get('status')=='completed', 'durable_status_'+str(row.get('status'))
                            assert row['assistant']['content']==expected, 'durable_answer_mismatch'
                            assert row['user']['content']==key, 'durable_prompt_mismatch'
                            checks['passed']+=1
                    except Exception as error:
                        checks[type(error).__name__]+=1
            await asyncio.gather(*(verify(index) for index in range(level)))
            if checks.get('passed',0)!=level:
                stage_errors['durable_check_failed']+=level-checks.get('passed',0)
            runtime = await runtime_snapshot()
            stage=dict(concurrency=level,completed=len(completed),queued=queued,
                elapsed_s=time.perf_counter()-began,errors=dict(stage_errors),error_samples=error_samples,first_text_ms=summary(first),
                completed_ms=summary(completed),probes=probes[stage_probes:],durable_checks=dict(checks),
                queued_final_snapshots=queued_snapshots,queued_rehydrated=len(deferred_checks),
                model_calls=model_after['calls']-model_before['calls'],model_peak=model_after['peak'],
                runtime_metrics=runtime)
            result['stages'].append(stage)
            (directory/('capacity-'+args.label+'.json')).write_text(json.dumps(result,indent=2),encoding='utf-8')
            print(json.dumps({key:value for key,value in stage.items()
                              if key not in ('probes', 'runtime_metrics')}),flush=True)
            if stage_errors:
                break
            await asyncio.sleep(3)
        stop.set()
        await observe
        result['probes']=probes
        result['closed_before_cleanup']=sum(socket is None or socket.closed for socket in sockets)
        result['passed']=not failures and not result['closed_before_cleanup'] and len(result['stages'])==len(args.levels.split(',')) and all(stage['completed']==stage['concurrency'] and not stage['errors'] for stage in result['stages']) and all(probe.get('status')==200 for probe in probes)
        await asyncio.gather(*(socket.close() for socket in sockets if socket is not None),return_exceptions=True)
        (directory/('capacity-'+args.label+'.json')).write_text(json.dumps(result,indent=2),encoding='utf-8')
        print(json.dumps(dict(passed=result['passed'],stages=len(result['stages']))),flush=True)

if __name__ == '__main__':
    asyncio.run(main())
