import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = fileURLToPath(new URL('../../../', import.meta.url));
const require = createRequire(resolve(repo, 'package.json'));
const { build } = require('esbuild');
const { parse, compileScript } = require('@vue/compiler-sfc');
const output = resolve(repo, 'temp_dir/frontend-tests/status-consistency-replay.cjs');
const source = `
import assert from 'node:assert/strict';
import { createRenderer, reactive, nextTick, h } from 'vue';
import Process from './src/components/chat/MessageTimelineTurnProcess.vue';
import { beginThreadView, commitThreadView, threadViewSlots, threadViewStatus, applyThreadViewHead, applyThreadViewChanges, clearThreadView } from './src/realtime/chat/threadViewStore';
import { resolveTaskRuntimeState } from './src/views/messenger/taskRuntimeState';

async function run() {
  const timers = new Map();
  let timerId = 0;
  globalThis.window = {
    setTimeout(callback) { const id = ++timerId; timers.set(id, callback); return id; },
    clearTimeout(id) { timers.delete(id); },
    requestAnimationFrame() { return ++timerId; },
    cancelAnimationFrame() {}
  };
  const drain = async () => { for (let index=0; index<12; index++) await nextTick(); };
  const tick = async () => { const callbacks=[...timers.values()]; timers.clear(); callbacks.forEach(callback=>callback()); await drain(); };
  const requests=[];
  globalThis.fixtureFetch = () => new Promise((resolve,reject) => requests.push({resolve,reject}));
  const row=(status='running', revision=1)=>({root_turn_id:'fixture-turn',execution_turn_id:'fixture-turn',user_round:1,revision,status,user:{item_id:'fixture-user',content:'fixture input'},assistant:{item_id:'fixture-answer',content:''},workflow_count:1,failed_count:0});
  const key='fixture-thread';
  const page={session_id:key,epoch:1,view_seq:1,projection_state:'ready',thread_empty:false,latest_round:1,chat:[row()],trajectory:[],tiles:[],tile_level:0,next_before:null,has_more:false,runtime_status:'running',status_turn_id:'fixture-turn',active_turn_id:'fixture-turn'};
  beginThreadView(key); commitThreadView(key,page);
  applyThreadViewHead(key,{epoch:1,view_seq:3,runtime_status:'completed',status_turn_id:'fixture-turn',active_turn_id:''});
  const headOnly={thread:threadViewStatus(key),bubble:threadViewSlots(key)[0].assistant.status,task:resolveTaskRuntimeState('not_loaded','running',false,threadViewStatus(key))};
  assert.deepEqual(headOnly,{thread:'completed',bubble:'completed',task:'done'});
  applyThreadViewChanges(key,{epoch:1,view_seq:2,runtime_status:'running',status_turn_id:'fixture-turn',active_turn_id:'fixture-turn',rows:[row('running',2)]});
  assert.equal(threadViewSlots(key)[0].assistant.status,'completed');
  clearThreadView(key);

  const makeNode=(type,text='')=>({type,text,props:{},children:[],parent:null,scrollHeight:300,clientHeight:100,scrollTop:200,getBoundingClientRect(){return {top:0,bottom:100};},querySelectorAll(){return [];}});
  const renderer=createRenderer({
    createElement:makeNode,createText:text=>makeNode('#text',text),createComment:text=>makeNode('#comment',text),
    setText(node,text){node.text=text;},setElementText(node,text){node.text=text;node.children=[];},
    patchProp(node,key,previous,next){node.props[key]=next;},parentNode:node=>node.parent,
    nextSibling(node){const siblings=node.parent?.children||[];return siblings[siblings.indexOf(node)+1]||null;},
    insert(node,parent,anchor){if(node.parent){const index=node.parent.children.indexOf(node);if(index>=0)node.parent.children.splice(index,1);}node.parent=parent;const index=anchor?parent.children.indexOf(anchor):-1;if(index<0)parent.children.push(node);else parent.children.splice(index,0,node);},
    remove(node){const index=node.parent?.children.indexOf(node)??-1;if(index>=0)node.parent.children.splice(index,1);},
    querySelector(){return null;},setScopeId(){},insertStaticContent(text,parent){const node=makeNode('#static',text);node.parent=parent;parent.children.push(node);return [node,node];}
  });
  const find=(node,predicate)=>predicate(node)?node:node.children.map(child=>find(child,predicate)).find(Boolean);
  const mount=()=>{
    const container=makeNode('root');
    const props=reactive({sessionId:key,turnId:'fixture-turn',count:1,status:'running',finalItemId:'fixture-answer',reasoningStreaming:false});
    const app=renderer.createApp({render(){return h(Process,props);}});
    app.mount(container);
    return {props,app,container,click:()=>find(container,node=>node.type==='button').props.onClick(),tool:()=>find(container,node=>node.type==='fixture-tool')?.props.status};
  };
  const toolItem=status=>({item_id:'fixture-call',item_index:1,turn_id:'fixture-turn',kind:'tool_call',status,revision:status==='running'?1:2,payload:{tool:'fixture_tool',tool_call_id:'fixture-call',args:{},...(status==='completed'?{result:'fixture result'}:{})}});
  const respond=(status,hasNewer=false)=>requests.shift().resolve({data:{data:{turn:{items:[toolItem(status)],has_older:false,has_newer:hasNewer}}}});
  const setup=async()=>{const fixture=mount();fixture.click();await drain();respond('running');await drain();assert.equal(fixture.tool(),'loading');return fixture;};

  let fixture=await setup();
  fixture.props.count=2;await drain();await tick();assert.equal(requests.length,1);
  fixture.props.status='completed';await drain();await tick();assert.equal(requests.length,1);
  respond('running');await drain();assert.equal(requests.length,1);
  respond('completed');await drain();assert.equal(fixture.tool(),'completed');assert.equal(requests.length,0);
  const repairedRefresh={bubble:fixture.props.status,tool:fixture.tool(),pendingRequests:requests.length,pendingTimers:timers.size};
  fixture.app.unmount();

  fixture=mount();fixture.click();await drain();
  fixture.props.status='completed';await drain();await tick();
  respond('running');await drain();assert.equal(requests.length,1);
  respond('completed');await drain();assert.equal(fixture.tool(),'completed');fixture.app.unmount();

  fixture=await setup();fixture.click();await drain();fixture.props.status='completed';await drain();await tick();
  assert.equal(requests.length,0);fixture.click();await drain();assert.equal(requests.length,1);
  respond('completed');await drain();assert.equal(fixture.tool(),'completed');fixture.app.unmount();

  fixture=await setup();fixture.props.count=2;await drain();await tick();
  fixture.props.status='completed';await drain();await tick();requests.shift().reject(new Error('fixture unavailable'));await drain();await tick();
  assert.equal(requests.length,0);fixture.props.count=3;await drain();await tick();
  respond('completed');await drain();assert.equal(fixture.tool(),'completed');fixture.app.unmount();

  fixture=mount();fixture.click();await drain();respond('running',true);await drain();
  fixture.props.status='completed';await drain();await tick();assert.equal(requests.length,0);
  const body=find(fixture.container,node=>node.props.class==='timeline-process-body');
  body.scrollHeight=body.clientHeight;
  body.props.onWheelPassive({deltaY:1});await drain();assert.equal(requests.length,1);
  respond('running');await drain();assert.equal(requests.length,1);
  respond('completed');await drain();assert.equal(fixture.tool(),'completed');fixture.app.unmount();

  fixture=await setup();fixture.props.count=2;await drain();await tick();
  fixture.props.status='completed';await drain();await tick();fixture.props.turnId='fixture-next-turn';await drain();
  respond('running');await drain();assert.equal(requests.length,0);assert.equal(fixture.tool(),undefined);fixture.app.unmount();
  assert.equal(timers.size,0);
  console.log(JSON.stringify({headOnly,repairedRefresh,initialRead:'pass',reopen:'pass',failure:'pass',history:'pass',identityReset:'pass'},null,2));
}
run().catch(error=>{console.error(error);process.exitCode=1;});
`;

await build({
  stdin: { contents: source, resolveDir: resolve(repo, 'frontend'), sourcefile: 'status-consistency-replay.ts', loader: 'ts' },
  outfile: output, bundle: true, platform: 'node', format: 'cjs', logLevel: 'silent',
  alias: { '@': resolve(repo, 'frontend/src') }, define: { 'import.meta.env': '{}' },
  plugins: [{ name: 'actual-process-component', setup(builder) {
    builder.onResolve({ filter: /^@\/api\/chat$/ }, () => ({ path: 'api', namespace: 'fixture' }));
    builder.onResolve({ filter: /^@\/i18n$/ }, () => ({ path: 'i18n', namespace: 'fixture' }));
    builder.onLoad({ filter: /.*/, namespace: 'fixture' }, args => ({ contents: args.path === 'api'
      ? 'export const getThreadLogTurn=(...args)=>globalThis.fixtureFetch(...args);'
      : 'export const t=(key)=>key;', loader: 'js' }));
    builder.onLoad({ filter: /\.vue$/ }, args => {
      if (args.path.endsWith('MessageTimelineTurnProcess.vue')) {
        const { descriptor } = parse(readFileSync(args.path, 'utf8'), { filename: args.path });
        const compiled = compileScript(descriptor, { id: 'fixture-process', inlineTemplate: true });
        return { contents: compiled.content, loader: 'ts', resolveDir: resolve(repo, 'frontend/src/components/chat') };
      }
      return { contents: args.path.endsWith('MessageTimelineToolEntry.vue')
        ? 'import {h} from "vue";export default {props:["entry"],setup(props){return ()=>h("fixture-tool",{status:props.entry.status});}};'
        : 'export default {render(){return null;}};', loader: 'js', resolveDir: resolve(repo, 'frontend') };
    });
  } }],
});
require(output);
