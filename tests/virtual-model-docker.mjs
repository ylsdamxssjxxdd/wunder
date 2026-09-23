// Isolated Linux x86 server acceptance: provider limits and a real read-only tool exchange.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
const base = "http://127.0.0.1:18000";
const { api_key } = JSON.parse(await readFile("temp_dir/throughput-docker/credentials.json", "utf8"));
const headers = {"X-API-Key":api_key,"Content-Type":"application/json"};
const api = async (path, body) => {
  const response = await fetch(`${base}/wunder${path}`, {headers, method:body === undefined ? "GET":"POST", ...(body === undefined ? {} : {body:JSON.stringify(body)}), signal:AbortSignal.timeout(90000)});
  const data = await response.json();
  return {response,data};
};
const original = (await api("/admin/llm")).data.llm;
const originalTools = (await api("/admin/tools")).data.enabled;
const originalLogs = (await api("/admin/llm/virtual_logs")).data.logs;
const modelName = "test_capabilities";
assert.equal(original.models[modelName],undefined);
const model = {...original.models.virtual, model:"", max_context:131072,max_output:1024, tool_call_mode:"function_call", simulation_speed:"fast", simulation:{support_reasoning:false,support_tools:true,image_tokens:384,audio_tokens:768}};
async function save() {
  const latest = (await api("/admin/llm")).data.llm;
  const {response,data} = await api("/admin/llm",{llm:{...latest,models:{...latest.models,[modelName]:model}}});
  assert.equal(response.status,200);
  assert.deepEqual(data.llm.models[modelName].simulation,model.simulation);
}
async function benchmark() {
  let started;
  for(let attempt=0;attempt<50;attempt++) {
    started = await api("/admin/throughput/start",{model_name:modelName,concurrency:2,input_tokens:1024,output_tokens:1024});
    if(started.response.status !== 409) break;
    await new Promise(resolve=>setTimeout(resolve,100));
  }
  assert.equal(started.response.status,200);
  for(let i=0;i<200;i++) {
    const {data} = await api("/admin/throughput/status");
    if(data.active?.id === started.data.id && !["running","stopping"].includes(data.active.status)) return data.active;
    await new Promise(resolve=>setTimeout(resolve,100));
  }
  throw new Error("Benchmark did not finish");
}
async function chat() {
  const response = await fetch(`${base}/wunder`,{method:"POST",headers,body:JSON.stringify({user_id:"test_virtual",question:"test",model_name:modelName,tool_names:["self_status"],stream:true,language:"en",debug_payload:true}),signal:AbortSignal.timeout(90000)});
  assert.equal(response.status,200);
  const body = await response.text();
  return body.split(/\r?\n\r?\n/).flatMap(block=> {
    const data = block.split(/\r?\n/).filter(line=>line.startsWith("data:")).map(line=>line.slice(5).trim()).join("\n");
    if(!data) return [];
    return [{event:block.match(/(?:^|\n)event:\s*(\S+)/)?.[1],...JSON.parse(data)}];
  });
}
const uploadedLogs = [];
try {
  assert.equal((await api("/admin/tools",{enabled:[...originalTools,"self_status"]})).response.status,200);
  for (const textual of [false, true]) {
    const call = {id:"call_1",name:"self_status",arguments:{detail_level:"basic",include_events:false,include_system_metrics:false}};
    const first = textual ? {content:`<tool_call>${JSON.stringify(call)}</tool_call>`} : {tool_calls:[{id:call.id,type:"function",function:{name:call.name,arguments:JSON.stringify(call.arguments)}}]};
    const log = [{role:"user",content:"test"},{role:"assistant",...first},{role:"assistant",content:"Recorded final reply."}].map(item=>JSON.stringify(item)).join("\n");
    const form = new FormData();
    form.append("name",`test_${textual ? "text" : "native"}`);
    form.append("file",new Blob([log],{type:"application/jsonl"}),"test.jsonl");
    const uploaded = await fetch(`${base}/wunder/admin/llm/virtual_logs`,{method:"POST",headers:{"X-API-Key":api_key},body:form});
    assert.equal(uploaded.status,200);
    model.model = (await uploaded.json()).log.id;
    uploadedLogs.push(model.model);
    for (const [mode,apiMode,native] of [["function_call","chat",true],["tool_call","chat",false],["freeform_call","chat",false],["freeform_call","responses",true]]) {
      model.tool_call_mode = mode;
      model.api_mode = apiMode;
      await save();
      const events = await chat();
      assert.equal(events.filter(event=>event.event === "error").length,0,JSON.stringify(events.filter(event=>event.event === "error")));
      const outputs = events.filter(event=>event.event === "llm_output").map(event=>event.data ?? event);
      assert.equal(outputs.length,2);
      assert.equal(outputs[0].finish_reason,native ? "tool_calls" : "stop");
      if (native) {
        assert.equal(outputs[0].tool_calls[0].function.name,"self_status");
        assert.equal(outputs[0].tool_calls[0].id,"call_1");
        assert.equal(outputs[0].content.includes("<tool_call>"),false);
      } else {
        assert.equal(outputs[0].tool_calls,null);
        assert.ok(outputs[0].content.includes("<tool_call>"));
      }
      assert.equal(outputs[1].finish_reason,"stop");
      assert.equal(outputs[1].content,"Recorded final reply.");
      assert.equal(outputs[1].tool_calls,null);
      assert.equal(outputs[1].reasoning,"");
      assert.equal(events.filter(event=>event.event === "tool_result").length,1);
      console.log(`PASS: ${textual ? "text" : "native"} replay through ${mode}/${apiMode}: one execution, then recorded final reply`);
    }
  }

  model.simulation.support_tools = false;
  await save();
  const rejected = await chat();
  assert.ok(JSON.stringify(rejected.filter(event=>event.event === "error")).includes("unsupported_tools"));
  assert.equal(rejected.filter(event=>event.event === "tool_result").length,0);
  console.log("PASS: unsupported tools return an error without execution");

  const valid = await benchmark();
  assert.equal(valid.status,"finished");
  assert.deepEqual([valid.config.concurrency,valid.metrics.output_tokens,valid.metrics.reasoning_tokens,valid.metrics.target_reached],[2,2048,0,true]);
  assert.ok(Math.abs(valid.metrics.decode_tps-200)<10);
  model.max_output = 512;
  await save();
  const tooLong = await benchmark();
  assert.equal(tooLong.status,"error");
  assert.ok(tooLong.error.includes("max_tokens_exceeded"));
  model.max_output = 1024;
  model.max_context = 2048;
  await save();
  // The actual generated prompt has 1025 estimated tokens; the coarse preset check passes.
  const context = await benchmark();
  assert.equal(context.status,"error");
  assert.ok(context.error.includes("context_length_exceeded"));
  console.log("PASS: reasoning disabled at 200 tok/s, output limit and actual-input context boundary");
  model.simulation.image_tokens = 0;
  const invalid = await api("/admin/llm",{llm:{...original,models:{...original.models,[modelName]:model}}});
  assert.equal(invalid.response.status,400);
  console.log("PASS: invalid simulation settings rejected at configuration API");
} finally {
  for (const id of uploadedLogs) {
    assert.equal((await fetch(`${base}/wunder/admin/llm/virtual_logs/${id}`,{method:"DELETE",headers})).status,200);
  }
  const finalLogs = (await api("/admin/llm/virtual_logs")).data.logs;
  assert.deepEqual(finalLogs.map(log=>log.id), originalLogs.map(log=>log.id));
  assert.equal((await api("/admin/tools",{enabled:originalTools})).response.status,200);
  const latest = (await api("/admin/llm")).data.llm;
  delete latest.models[modelName];
  assert.equal((await api("/admin/llm",{llm:latest})).response.status,200);
}
