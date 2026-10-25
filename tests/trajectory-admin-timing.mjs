// Run: node tests/trajectory-admin-timing.mjs. Owned static browser fixture only.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { chromium } from "playwright";

const source = await readFile(new URL("../web/modules/trajectory-model.js", import.meta.url), "utf8");
const model = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
const translate = (key, values) => `${key}${values ? JSON.stringify(values) : ""}`;
const instant = 1800000100;
const row = (id, kind, payload = {}, fields = {}) => ({
  item_id: id, turn_id: "sample-turn", created_seq: 1, kind, status: "completed",
  created_time: instant, updated_time: instant, payload, ...fields,
});
const cell = (item) => model.buildTrajectoryLayout([{ user_round: 1, items: [item] }], translate)[0].groups[0].cells[0];

const modelItem = row("sample-model", "assistant_message", {
  content: "sample output", prefill_duration_s: .5, decode_duration_s: 1.75,
  message_stats: { interaction_duration_s: 90 },
});
assert.equal(cell(modelItem).timeSeconds, 2.25);
assert.equal(cell(modelItem).startedAt, instant * 1000 - 2250);
assert.equal(cell(modelItem).metrics.timingRecorded, true);
assert.equal(cell(row("sample-stats", "assistant_message", {
  content: "sample output", stats: { model_duration_s: "1.125" },
})).timeSeconds, 1.125);
assert.equal(cell(row("sample-stream", "assistant_message", {
  stream_timing: { prefill_ms: 125, decode_ms: 375 },
})).timeSeconds, .5);
assert.equal(cell(row("sample-tool", "tool_call", { tool: "sample-tool", meta: { duration_ms: 725 } })).timeSeconds, .725);
assert.equal(cell(row("sample-seconds", "tool_call", { tool: "sample-tool", elapsed_s: 1.5 })).timeSeconds, 1.5);

for (const duration of [undefined, null, "", " ", -1]) {
  const unknown = cell(row("sample-unknown", "tool_call", { tool: "sample-tool", duration_ms: duration }));
  assert.equal(unknown.timeSeconds, null, "unknown duration must not become zero");
}
assert.equal(cell(row("sample-zero", "tool_call", { tool: "sample-tool", duration_ms: 0 })).timeSeconds, 0);
assert.equal(cell(row("sample-explicit-zero", "tool_call", { tool: "sample-tool" }, {
  started_at: instant, completed_at: instant,
})).timeSeconds, 0);
assert.equal(cell(row("sample-span", "assistant_message", { stats: { model_duration_s: 90 } }, {
  started_at: instant - 2, completed_at: instant,
})).timeSeconds, 2, "backend seconds interval is authoritative");

const wrapped = row("sample-wrapped", "configuration", {
  item_id: "sample-wrapped", kind: "tool_call", status: "completed", payload: {
    tool: "sample-tool", meta: { duration_ms: 450 }, content: "sample result",
  },
});
assert.equal(cell(wrapped).kind, "tool");
assert.equal(cell(wrapped).timeSeconds, .45);
assert.equal(cell(wrapped).toolName, "sample-tool");

const call = row("sample-call", "tool_call", { tool: "sample-tool", tool_call_id: "sample-call-id" }, {
  status: "running", created_time: instant - 1, created_seq: 2,
});
const result = row("sample-result", "tool_result", {
  tool_call_id: "sample-call-id", meta: { duration_ms: 650 }, data: { content: "sample result" },
}, { created_seq: 3 });
const merged = model.flattenRecords(model.buildTrajectoryLayout([{ user_round: 1, items: [call, result] }], translate));
assert.equal(merged.length, 1);
assert.equal(merged[0].cell.timeSeconds, .65);
const timed = model.deriveTimeline(model.buildTrajectoryLayout([{ user_round: 1, items: [modelItem] }], translate), "duration");
assert.equal(timed.end - timed.start, 2250);

const root = path.resolve("web");
const server = createServer(async (request, response) => {
  const name = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
  const file = path.resolve(root, `.${name === "/" ? "/index.html" : name}`);
  if (!file.startsWith(`${root}${path.sep}`)) { response.writeHead(403).end(); return; }
  try {
    let body = await readFile(file);
    if (file.endsWith("index.html")) body = Buffer.from(body.toString().replace(/<script type="module" src="\.\/app\.js[^>]*><\/script>/, ""));
    const mime = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml" }[path.extname(file)] || "application/octet-stream";
    response.writeHead(200, { "Content-Type": `${mime}; charset=utf-8` }).end(body);
  } catch { response.writeHead(404).end(); }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
let browser;
try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.name));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.evaluate(async ({ modelItem, wrapped, unknown }) => {
    document.body.replaceChildren();
    const host = document.createElement("div");
    host.style.cssText = "height:850px;width:1200px;display:flex;flex-direction:column";
    document.body.append(host);
    const { createTrajectoryView } = await import("/modules/trajectory-view.js");
    globalThis.sampleTrajectory = createTrajectoryView(host);
    globalThis.sampleTrajectory.setRawTurns([{ user_round: 1, items: [modelItem, { ...wrapped, created_seq: 2 }, { ...unknown, created_seq: 3 }] }]);
  }, { modelItem, wrapped, unknown: row("sample-unknown-ui", "tool_call", { tool: "sample-unknown-tool" }) });
  await page.locator('.tt-row[data-record-index="1"]').click();
  const inspector = page.locator(".tt-details");
  await inspector.waitFor({ state: "visible" });
  await inspector.locator(".tt-detail-tab").first().click();
  await inspector.locator("dt").first().waitFor();
  const durations = await inspector.locator(".tt-overview-row").evaluateAll((rows) => rows.filter((item) =>
    ["时长", "Duration"].includes(item.querySelector("dt")?.textContent)).map((item) => item.querySelector("dd")?.textContent));
  assert.equal(durations.length, 1, "tool inspector must display one duration field");
  assert.match(durations[0], /450/);
  const widths = await page.locator(".tt-span-message, .tt-span-tool").evaluateAll((nodes) => nodes.map((node) => parseFloat(node.style.width)));
  assert.ok(widths[0] > 10 && widths[1] > 10, "recorded model/tool intervals must paint real spans");
  await page.locator('.tt-row[data-record-index="2"]').click();
  await inspector.locator(".tt-detail-tab").first().click();
  await inspector.locator("dt").first().waitFor();
  const unknownDuration = await inspector.locator(".tt-overview-row").evaluateAll((rows) => rows.filter((item) =>
    ["时长", "Duration"].includes(item.querySelector("dt")?.textContent)).map((item) => item.querySelector("dd")?.textContent));
  assert.equal(unknownDuration.length, 1);
  assert.doesNotMatch(unknownDuration[0], /^0(?:\s|$)/);
  assert.deepEqual(errors, []);
  console.log("trajectory admin timing: model, units, canonical envelopes, merge, unknown/zero and browser inspector passed");
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
