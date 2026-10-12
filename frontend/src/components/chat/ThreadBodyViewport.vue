<template>
  <div class="thread-body-viewport">
    <MessageMarkdownBody v-if="!expanded" :cache-key="cacheKey" :content="preview" :explicit-content="true" />
    <div v-else class="thread-body-scroll"><pre>{{ text }}</pre></div>
    <div class="thread-body-controls">
      <button v-if="!expanded" :disabled="loading" type="button" @click="load(0)">查看完整正文</button>
      <template v-else>
        <button :disabled="loading || cursor === 0" @click="first">回到开头</button>
        <button :disabled="loading || !previous.length" @click="back">上一页</button>
        <button :disabled="loading || !more" @click="forward">下一页</button>
      </template>
      <span v-if="loading" role="status">加载中…</span>
      <span v-if="error" role="alert">{{ error }}</span>
    </div>
  </div>
</template>
<script setup lang="ts">
import {ref, onBeforeUnmount, watch} from 'vue';
import MessageMarkdownBody from './MessageMarkdownBody.vue';
import {getThreadViewBody} from '@/api/chat';
import type {ThreadBodyRef} from '@/realtime/chat/threadViewStore';
const props=defineProps<{sessionId:string;body:ThreadBodyRef;preview:string;cacheKey:string}>();
const expanded=ref(false), loading=ref(false), more=ref(true), error=ref(''), text=ref(''), cursor=ref(0), previous=ref<number[]>([]);
let next=0, abort:AbortController|null=null;
// A single bounded plain-text page avoids parsing or retaining a giant Markdown body.
async function load(target:number):Promise<boolean> {
  abort?.abort();const request=new AbortController();abort=request;loading.value=true;error.value='';
  try {
    const r=await getThreadViewBody(props.sessionId,props.body.body_id,{revision:props.body.revision,cursor:target},{signal:request.signal});
    if(request.signal.aborted || abort!==request)return false;
    const p=r.data.data;if(p.has_more && (!Number.isSafeInteger(p.next_cursor)||p.next_cursor<=target))throw new Error('invalid body cursor');
    text.value=p.chunks.join('');cursor.value=target;next=p.next_cursor??0;more.value=p.has_more;expanded.value=true;return true;
  } catch(e) {if(!request.signal.aborted)error.value='正文读取失败，请重试';return false;}
  finally {if(abort===request){loading.value=false;abort=null;}}
}
async function forward(){const prior=cursor.value;if(await load(next))previous.value=[...previous.value,prior].slice(-64);}
async function back(){const target=previous.value.at(-1);if(target!==undefined && await load(target))previous.value=previous.value.slice(0,-1);}
async function first(){if(await load(0))previous.value=[];}
watch(()=>`${props.sessionId}:${props.body.body_id}:${props.body.revision}`,()=>{abort?.abort();abort=null;loading.value=false;text.value='';cursor.value=0;previous.value=[];expanded.value=false;more.value=true;});
onBeforeUnmount(()=>abort?.abort());
</script>
<style scoped>
.thread-body-scroll{max-height:480px;overflow:auto}.thread-body-scroll pre{margin:0;white-space:pre-wrap;font:inherit;overflow-wrap:anywhere}.thread-body-controls{display:flex;gap:8px;align-items:center;flex-wrap:wrap}.thread-body-controls button{margin:8px 0;border:1px solid var(--border-color,#ddd);background:transparent;border-radius:6px;padding:4px 10px;cursor:pointer}
</style>
