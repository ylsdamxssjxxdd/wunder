import {getThreadViewBody} from '@/api/chat';
import type {ThreadBodyRef} from './threadViewStore';
export async function readThreadBody(sessionId:string,body:ThreadBodyRef,signal?:AbortSignal):Promise<string> {
  if(body.body_id.endsWith('|stream') && body.revision===0)throw new Error('输出完成后可复制完整正文');
  const parts:string[]=[];let cursor=0;
  for(;;){const r=await getThreadViewBody(sessionId,body.body_id,{revision:body.revision,cursor},{signal});const p=r.data.data;
    parts.push(...p.chunks);if(!p.has_more)break;
    if(!Number.isSafeInteger(p.next_cursor)||p.next_cursor<=cursor)throw new Error('invalid body cursor');cursor=p.next_cursor;
  }
  return parts.join('');
}
