import { getChatDebugSnapshot } from './chatDebug';
import { chatPerf } from './chatPerf';

// Export diagnostic structure and timings without copying arbitrary chat/tool bodies.
const PRIVATE_KEY = /(?:content|text|prompt|reasoning|payload|raw|detail|args|arguments|output|result|token|secret|password|authorization|cookie|path|url|uri|username|email|id$)/i;
const NUMERIC_METRIC = /(?:count|length|size|bytes|ms|duration|tokens)$/i;
const STATE_KEY = /^(?:status|state|stage|source|reason|role|kind|type|event|eventType|phase|mode|toolName|label|channel|operation|action)$/i;

const summarizePayload = (value: unknown, depth = 0, budget = { remaining: 128 }): unknown => {
  if (--budget.remaining < 0 || depth > 4) return '[truncated]';
  if (value === null || typeof value === 'boolean' || typeof value === 'number') return value;
  if (typeof value === 'string') return '[omitted]';
  if (Array.isArray(value)) return value.slice(0, 16).map(item => summarizePayload(item, depth + 1, budget));
  if (!value || typeof value !== 'object') return undefined;
  const result: Record<string, unknown> = {};
  for (const key of Object.keys(value).slice(0, 32)) {
    if (budget.remaining <= 0) break;
    const item = (value as Record<string, unknown>)[key];
    if (PRIVATE_KEY.test(key) && !(typeof item === 'number' && NUMERIC_METRIC.test(key))) continue;
    if (typeof item === 'string') {
      if (STATE_KEY.test(key) && /^[a-zA-Z0-9_.:-]{1,120}$/.test(item)) result[key] = item;
      continue;
    }
    result[key] = summarizePayload(item, depth + 1, budget);
  }
  return result;
};

export const buildChatDebugReport = () => {
  const debug = getChatDebugSnapshot();
  return {
    schemaVersion: 1,
    generatedAt: new Date().toISOString(),
    debug: {
      enabled: debug.enabled,
      verbose: debug.verbose,
      events: debug.events.map(entry => ({
        time: entry.time,
        scope: entry.scope,
        event: entry.event,
        payload: summarizePayload(entry.payload)
      }))
    },
    performance: chatPerf.snapshot()
  };
};

export const downloadChatDebugReport = () => {
  // Construct on every click: neither source is Vue reactive, so a computed would go stale.
  const report = buildChatDebugReport();
  const url = URL.createObjectURL(new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' }));
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `wunder-debug-${report.generatedAt.replace(/[:.]/g, '-')}.json`;
  try {
    document.body.appendChild(anchor);
    anchor.click();
  } finally {
    anchor.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
};
