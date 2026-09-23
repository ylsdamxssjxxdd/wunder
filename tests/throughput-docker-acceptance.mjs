// Requires packaging/docker/docker-compose-throughput.yml; performs no external model calls.
import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { verifyBrowser } from "./throughput-docker-browser.mjs";

const base = "http://127.0.0.1:18000";
const config = {security: JSON.parse(await readFile("temp_dir/throughput-docker/credentials.json", "utf8"))};
const headers = { "X-API-Key": config.security.api_key, "Content-Type": "application/json" };
const results = [];
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function api(path, body, status = 200) {
  const response = await fetch(`${base}/wunder/admin/throughput/${path}`, { headers, method: body === undefined ? "GET" : "POST", ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
  assert.equal(response.status, status, `endpoint ${path}`);
  return response.json();
}
function counts() {
  const sql = "SELECT json_build_object('users',(SELECT count(*) FROM user_accounts),'sessions',(SELECT count(*) FROM chat_sessions),'history',(SELECT count(*) FROM chat_history),'monitor',(SELECT count(*) FROM monitor_sessions),'stream',(SELECT count(*) FROM stream_events));";
  return JSON.parse(execFileSync("docker",["exec","wunder-throughput-postgres-1","psql","-U","test","-d","test","-At","-c",sql],{encoding:"utf8"}).trim());
}
async function waitFinished(id, timeoutMs = 180000) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const {active} = await api("status");
    if (active?.id === id && !["running","stopping"].includes(active.status)) {
      await sleep(80);
      return active;
    }
    await sleep(150);
  }
  throw new Error("Test did not settle");
}
async function run(model, input, output = 1024) {
  const active = await api("start", {model_name:model,input_tokens:input,output_tokens:output});
  const result = await waitFinished(active.id);
  results.push(result);
  console.log(JSON.stringify({model,input,output,status:result.status,ttft_ms:result.metrics.ttft_ms,prefill_tps:result.metrics.prefill_tps,decode_tps:result.metrics.decode_tps,actual_output:result.metrics.output_tokens}));
  return result;
}
function validTiming(result, prefill = 2000, generation = 200) {
  const {metrics} = result;
  assert.equal(result.status,"finished");
  assert.equal(metrics.target_reached,true);
  assert.equal(metrics.output_tokens,result.config.output_tokens);
  assert.equal(metrics.reasoning_tokens, result.config.output_tokens / 4);
  const expected = metrics.input_tokens / prefill * 1000;
  assert.ok(metrics.ttft_ms >= expected - 5 && metrics.ttft_ms < expected + 600, `prefill expected ${expected}, got ${metrics.ttft_ms}`);
  assert.ok(Math.abs(metrics.decode_tps / generation - 1) < 0.05, "decode speed");
}

// Wait for the fresh release binary, not merely an open port.
let ready = false;
for (let attempt=0;attempt<180;attempt++) {
  try { const status = await api("status"); if (status.schema_version === 2) { ready=true;break; } } catch {}
  await sleep(1000);
}
assert.ok(ready,"Docker service did not become ready");
if (process.argv.includes("--verify-history")) {
  const previous = JSON.parse(await readFile("temp_dir/throughput-docker/results.json","utf8"));
  const current = await api("status");
  assert.equal(current.active,null,"A restarted service must not resume an old request");
  // JSON readers may differ by one floating-point ULP; preserve all meaningful precision.
  const normalized = value => JSON.parse(JSON.stringify(value, (_,item) =>
    typeof item === "number" && !Number.isInteger(item) ? Number(item.toPrecision(12)) : item));
  for (const record of previous.results) assert.deepEqual(normalized(current.history.find(item=>item.id===record.id)),normalized(record));
  assert.deepEqual(counts(),previous.counts_after);
  console.log("PASS: restart restores identical performance summaries without thread records");
  process.exit(0);
}
const before = counts();
assert.equal((await fetch(`${base}/wunder/admin/throughput/status`)).status,401);
await api("start",{model_name:"virtual",input_tokens:3,output_tokens:1024},400);
await api("report?run_id=..%2Finvalid",undefined,404);

const {ticket} = await api("ticket",{});
const connection = new WebSocket(`${base.replace("http:","ws:")}/wunder/throughput/ws`,["wunder-throughput",`ticket.${ticket}`]);
let frames = 0;
let previousSequence = 0;
connection.addEventListener("message",({data})=>{
  const event = JSON.parse(String(data));
  assert.equal(event.event,"snapshot"); assert.ok(event.sequence>previousSequence);
  previousSequence=event.sequence;frames+=1;
});
await new Promise((resolve,reject)=>{connection.addEventListener("open",resolve,{once:true});connection.addEventListener("error",reject,{once:true});});
const reused = new WebSocket(`${base.replace("http:","ws:")}/wunder/throughput/ws`,["wunder-throughput",`ticket.${ticket}`]);
await new Promise((resolve,reject)=>{reused.addEventListener("error",resolve,{once:true});reused.addEventListener("open",()=>reject(new Error("Single-use ticket was accepted twice")),{once:true});});

validTiming(await run("virtual",1024));
validTiming(await run("virtual",8192));
validTiming(await run("virtual_medium",1024),500,50);
validTiming(await run("virtual_slow",1024),100,10);
validTiming(await run("api",2048,2048));
const mockMetrics = JSON.parse(execFileSync("docker", ["exec", "wunder-throughput-model-1", "python3", "-c", "import urllib.request; print(urllib.request.urlopen('http://127.0.0.1:18090/metrics').read().decode())"], {encoding:"utf8"}));
assert.deepEqual([mockMetrics.last.output_target,mockMetrics.last.min_tokens,mockMetrics.last.ignore_eos],[2048,2048,true]);
const short=await run("short",1024);
assert.deepEqual([short.status,short.metrics.output_tokens,short.metrics.target_reached],["incomplete",64,false]);
const dropped=await run("disconnect",1024);
assert.equal(dropped.status,"error");
const rejectedControls=await run("reject",1024);
assert.equal(rejectedControls.status,"error");
const missing=await run("missing",1024);
assert.deepEqual([missing.status,missing.metrics.output_tokens,missing.metrics.target_reached],["incomplete",null,null]);

for(const model of ["virtual","api"]) {
  const active = await api("start",{model_name:model,input_tokens:1048576,output_tokens:1024});
  await sleep(200);
  await api("start",{model_name:model,input_tokens:1024,output_tokens:1024},409);
  const inPrefill=(await api("status")).active;
  assert.equal(inPrefill.metrics.ttft_ms,null);
  const started=Date.now();await api("stop",{});
  const stopped=await waitFinished(active.id,2000);
  assert.ok(Date.now()-started<2000);assert.equal(stopped.status,"stopped");
  results.push(stopped);
}
connection.close();
assert.ok(frames>10,"real WebSocket snapshots");
const after=counts();assert.deepEqual(after,before,"benchmark must not create users, threads, messages or stream logs");
const status=await api("status");
for(const result of results) assert.ok(status.history.some((item)=>item.id===result.id));
const saved=JSON.parse(await readFile("temp_dir/throughput-docker/runtime/config/data/throughput/scenarios-v2.json","utf8"));
assert.equal(saved.length,status.history.length);
assert.ok(saved.every((item)=>!("messages" in item) && !("content" in item) && !("api_key" in item)));

await verifyBrowser(results);
await writeFile("temp_dir/throughput-docker/results.json",JSON.stringify({architecture:"linux/amd64",profiles:{fast:[2000,200],medium:[500,50],slow:[100,10]},counts_before:before,counts_after:after,websocket_frames:frames,results},null,2));
console.log("PASS: Docker model timing, fixed output controls, incomplete/error handling, cancellation, real WebSocket, no thread persistence and browser history curves");
