// AI生成
/**
 * collect-web-metrics.mjs
 *
 * Web-form metrics via Playwright + CDP.
 *
 * Modes:
 *   startup  median navigation time to DOMContentLoaded (ms)
 *   memory   JS heap used size after load (MB)
 *   cpu      renderer main-thread TaskDuration rate over a window (% of one core)
 *
 * Prints a single JSON line to stdout: { mode, url, value, unit, details } or
 * { mode, url, value: null, error }. Always exits 0 so the caller can record the
 * failure instead of aborting the whole benchmark.
 *
 * Usage:
 *   node scripts/form-bench/collect-web-metrics.mjs --mode startup --url http://127.0.0.1:5173/ --runs 5 --warmup 1
 */

import process from 'node:process';

function parseArgs(argv) {
  const out = { mode: 'startup', url: '', runs: 5, warmup: 1, durationSec: 20 };
  for (let i = 0; i < argv.length; i += 1) {
    const cur = argv[i];
    const next = argv[i + 1];
    const take = () => { i += 1; return next; };
    if (cur === '--mode') out.mode = take();
    else if (cur.startsWith('--mode=')) out.mode = cur.slice('--mode='.length);
    else if (cur === '--url') out.url = take();
    else if (cur.startsWith('--url=')) out.url = cur.slice('--url='.length);
    else if (cur === '--runs') out.runs = Number.parseInt(take(), 10);
    else if (cur === '--warmup') out.warmup = Number.parseInt(take(), 10);
    else if (cur === '--duration-sec') out.durationSec = Number(take());
  }
  return out;
}

async function loadChromium() {
  try {
    const mod = await import('playwright');
    return mod.chromium;
  } catch {
    const mod = await import('@playwright/test');
    return mod.chromium;
  }
}

function median(nums) {
  const a = [...nums].sort((x, y) => x - y);
  if (!a.length) return null;
  const n = a.length;
  return n % 2 ? a[(n - 1) / 2] : (a[n / 2 - 1] + a[n / 2]) / 2;
}

function round(value, digits) {
  if (value == null) return null;
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}

async function runStartup(chromium, args) {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  const samples = [];
  const total = Math.max(1, args.runs + args.warmup);
  for (let i = 0; i < total; i += 1) {
    const t0 = Date.now();
    await page.goto(args.url, { waitUntil: 'domcontentloaded', timeout: 60000 });
    const elapsed = Date.now() - t0;
    if (i >= args.warmup) samples.push(elapsed);
  }
  await browser.close();
  return {
    value: round(median(samples), 1),
    unit: 'ms',
    details: {
      mode: 'startup',
      median_ms: round(median(samples), 1),
      min_ms: samples.length ? Math.min(...samples) : null,
      max_ms: samples.length ? Math.max(...samples) : null,
      valid_runs: samples.length,
      samples
    }
  };
}

async function readPerfMetrics(client) {
  const res = await client.send('Performance.getMetrics');
  const out = {};
  for (const m of res.metrics) out[m.name] = m.value;
  return out;
}

async function runMemory(chromium, args) {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  await page.goto(args.url, { waitUntil: 'load', timeout: 60000 });
  const client = await page.context().newCDPSession(page);
  await client.send('Performance.enable');
  const metrics = await readPerfMetrics(client);
  const heapBytes = Number(metrics.JSHeapUsedSize || 0);
  const nodes = Number(metrics.Nodes || 0);
  await browser.close();
  return {
    value: round(heapBytes / 1024 / 1024, 2),
    unit: 'MB',
    details: {
      mode: 'memory',
      heap_used_mb: round(heapBytes / 1024 / 1024, 2),
      heap_total_mb: round(Number(metrics.JSHeapTotalSize || 0) / 1024 / 1024, 2),
      dom_nodes: nodes,
      note: 'JS heap of the page renderer (browser process memory excluded)'
    }
  };
}

async function runCpu(chromium, args) {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  await page.goto(args.url, { waitUntil: 'load', timeout: 60000 });
  const client = await page.context().newCDPSession(page);
  await client.send('Performance.enable');
  const before = await readPerfMetrics(client);
  const t0 = Date.now();
  await new Promise((resolve) => setTimeout(resolve, args.durationSec * 1000));
  const after = await readPerfMetrics(client);
  const wall = (Date.now() - t0) / 1000;
  await browser.close();
  const taskDelta = Number(after.TaskDuration || 0) - Number(before.TaskDuration || 0);
  const pct = wall > 0 ? (taskDelta / wall) * 100 : 0;
  return {
    value: round(pct, 2),
    unit: '%',
    details: {
      mode: 'cpu',
      percent_of_one_core: round(pct, 2),
      task_duration_sec: round(taskDelta, 3),
      wall_sec: round(wall, 3),
      script_duration_sec: round(Number(after.ScriptDuration || 0) - Number(before.ScriptDuration || 0), 3),
      note: 'renderer main-thread TaskDuration rate'
    }
  };
}

const args = parseArgs(process.argv.slice(2));

(async () => {
  try {
    if (!args.url) throw new Error('missing --url');
    const chromium = await loadChromium();
    let result;
    if (args.mode === 'startup') result = await runStartup(chromium, args);
    else if (args.mode === 'memory') result = await runMemory(chromium, args);
    else if (args.mode === 'cpu') result = await runCpu(chromium, args);
    else throw new Error(`unknown mode: ${args.mode}`);
    process.stdout.write(`${JSON.stringify({ mode: args.mode, url: args.url, ...result })}\n`);
  } catch (err) {
    process.stdout.write(`${JSON.stringify({
      mode: args.mode,
      url: args.url,
      value: null,
      error: String((err && err.message) || err)
    })}\n`);
  }
})();