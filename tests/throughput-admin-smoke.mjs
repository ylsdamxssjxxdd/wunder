// Run: node tests/throughput-admin-smoke.mjs (no live model or server required).
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright";

const root = path.resolve("web");
const server = createServer(async (request, response) => {
  const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
  const file = path.resolve(root, `.${name === "/" ? "/index.html" : name}`);
  if (!file.startsWith(`${root}${path.sep}`)) { response.writeHead(403).end(); return; }
  try {
    let body = await readFile(file);
    if (file.endsWith("index.html")) body = Buffer.from(body.toString().replace(/<script type="module" src="\.\/app\.js[^>]*><\/script>/, ""));
    const type = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml" }[path.extname(file)] || "application/octet-stream";
    response.writeHead(200, { "Content-Type": `${type}; charset=utf-8` }).end(body);
  } catch { response.writeHead(404).end(); }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 1080 } });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const makeRun = (id, input, speed, status = "finished") => ({
    id, status, config: { model_name: "model", concurrency: 1, input_tokens: input, output_tokens: 1024 }, samples: [],
    started_at: `2026-01-01T00:0${id}:00Z`, finished_at: "2026-01-01T00:10:00Z", elapsed_s: 12,
    length_control: "best_effort", persistence_error: false, error: null,
    metrics: { input_tokens: input + 10, output_tokens: status === "finished" ? 1024 : 70, reasoning_tokens: null, estimated_output_tokens: 1030,
      ttft_ms: 240, max_ttft_ms: 240, decode_tps: speed, avg_decode_tps: speed, prefill_tps: input / 0.24, avg_prefill_tps: input / 0.24, target_reached: status === "finished", finish_reason: "stop" },
  });
  let snapshot = { schema_version: 2, active: null, history: [makeRun("1",1024,94),makeRun("2",8192,82),makeRun("3",16384,75,"incomplete")] };
  let ws = null;
  let sequence = 0;
  let starts = [];
  let failStart = false;
  const publish = () => ws?.send(JSON.stringify({ event: "snapshot", sequence: ++sequence, data: snapshot }));
  await page.routeWebSocket(/\/wunder\/throughput\/ws/, (connection) => { ws = connection; sequence = 0; publish(); });
  await page.route("**/wunder/admin/throughput/**", async (route) => {
    const endpoint = new URL(route.request().url()).pathname.split("/").at(-1);
    let body = snapshot;
    if (endpoint === "ticket") body = { ticket: "temporary", expires_in_s: 30 };
    if (endpoint === "start") {
      if (failStart) { await route.fulfill({ status: 400, json: { error: { message: "长度配置无效" } } }); return; }
      const config = route.request().postDataJSON(); starts.push(config);
      snapshot.active = { ...makeRun("4", config.input_tokens, 0, "running"), config, finished_at: null };
      body = snapshot.active;
    }
    if (endpoint === "stop") { snapshot.active.status = "stopped"; body = snapshot.active; }
    await route.fulfill({ json: body });
  });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.evaluate(async () => {
    document.querySelectorAll(".panel.active").forEach((panel) => panel.classList.remove("active"));
    document.getElementById("throughputPanel").classList.add("active");
    const { state } = await import("/modules/state.js");
    state.llm = { ...state.llm, loaded: true, order: ["model"], defaultName: "model", configs: { model: { enable: true, model_type: "llm", max_context: 32768 } } };
    state.runtime.activePanel = "throughput";
    window.throughput = await import("/modules/throughput.js?v=20260925-07");
    await window.throughput.initThroughputPanel();
  });
  await page.waitForFunction(() => document.querySelectorAll("#tpHistory tr").length === 3);
  assert.equal(await page.locator("#tpInputPresets option").count(), 10);
  assert.equal(await page.locator("#tpOutputPresets option").count(), 4);
  assert.equal(await page.locator("#tpConcurrencyList").inputValue(), "1,2,4,8");
  assert.equal(await page.locator("#tpInput").inputValue(), "8192");
  assert.equal(await page.locator("#tpOutput").inputValue(), "128");
  assert.equal(await page.locator("#tpForm .muted").count(), 0);
  assert.match(await page.locator('#tpInput').locator('..').getAttribute("title"), /1,000/);
  assert.equal(await page.locator(".tp-help, .tp-retention").count(), 0);
  assert.equal(await page.locator("#tpHistory input:checked").count(), 3);
  const series = () => page.evaluate(() => window.echarts.getInstanceByDom(document.getElementById("tpChart")).getOption().series);
  assert.equal((await series()).length, 4);
  await page.locator('#tpHistory input[data-select="3"]').check();
  assert.equal((await series()).length, 4);
  await page.locator("#tpClear").click();
  assert.equal(await page.locator("#tpHistory input:checked").count(), 0);
  await page.locator("#tpSelectValid").click();
  assert.equal(await page.evaluate(() => window.echarts.getInstanceByDom(document.getElementById("tpChart")).getOption().xAxis[0].name), "并发数");
  assert.equal(await page.evaluate(() => window.echarts.getInstanceByDom(document.getElementById("tpChart")).getOption().yAxis[0].axisLabel.formatter(25)), "+25%");
  await page.locator('[data-view="2"]').click();
  assert.match(await page.locator("#tpDetail").innerText(), /8.2k/);
  assert.match(await page.locator("#tpDetail").innerText(), /单预填充速度/);
  assert.match(await page.locator("#tpDetail").innerText(), /单生成速度/);
  await page.locator("#tpInput").fill("1048576");
  await page.locator("#tpStart").click();
  await page.waitForFunction(() => document.getElementById("tpFeedback").textContent.includes("上下文"));
  assert.equal(starts.length, 0);
  await page.locator("#tpConcurrencyList").fill("1, 4, 4");
  await page.locator("#tpInput").fill("8192");
  await page.locator("#tpOutput").fill("2048");
  await page.locator("#tpStart").click();
  await page.locator("#tpStop").waitFor({ state: "visible" });
  assert.deepEqual(starts, [{model_name:"model",concurrency_list:[1,4],input_tokens:8192,output_tokens:2048}]);
  assert.equal(await page.locator("#tpModel").isDisabled(), true);
  snapshot.active.status = "finished";
  snapshot.active.finished_at = "2026-01-01T00:10:00Z";
  snapshot.active.samples = [
    { concurrency: 1, status: "finished", elapsed_s: 1, metrics: snapshot.active.metrics },
    { concurrency: 4, status: "finished", elapsed_s: 1, metrics: { ...snapshot.active.metrics, decode_tps: 300, avg_decode_tps: 80 } },
  ];
  snapshot.history.push(snapshot.active); publish();
  await page.waitForFunction(() => document.querySelectorAll("#tpHistory tr").length === 4);
  assert.equal(starts.length, 1);
  await page.locator("#tpStart").waitFor({ state: "visible" });
  failStart = true;
  await page.locator("#tpStart").click();
  await page.waitForFunction(() => document.getElementById("tpFeedback").textContent.includes("长度配置无效"));
  assert.equal(await page.locator("#tpHistory input:checked").count(), 4);
  await page.locator('[data-view="3"]').click();
  assert.match(await page.locator("#tpDetail").innerText(), /已记录/);
  await page.locator("#tpCurrent").click();
  await mkdir("temp_dir", { recursive: true });
  await page.screenshot({ path: "temp_dir/throughput-admin-desktop.png", fullPage: true });
  const download = page.waitForEvent("download");
  await page.locator("#tpExport").click();
  assert.equal((await download).suggestedFilename(), "throughput-comparison.json");
  await page.evaluate(async () => { const {setLanguage} = await import("/modules/i18n.js?v=20260710-01"); setLanguage("en-US"); });
  assert.equal(await page.locator("#tpStart").innerText(), "Start test");
  await page.setViewportSize({ width: 720, height: 980 });
  await page.locator("#tpStart").scrollIntoViewIfNeeded();
  const box = await page.locator("#tpStart").boundingBox();
  assert.ok(box.x >= 0 && box.x + box.width <= 720);
  await page.screenshot({ path: "temp_dir/throughput-admin-narrow.png", fullPage: true });
  await page.evaluate(() => window.throughput.toggleThroughputPolling(false));
  assert.deepEqual(errors, []);
  console.log("PASS: presets, concurrency-list payload and sequencing, context validation, stop, API errors, live snapshots, curves, export, locale and narrow layout");
} finally {
  await browser.close(); await new Promise((resolve) => server.close(resolve));
}
