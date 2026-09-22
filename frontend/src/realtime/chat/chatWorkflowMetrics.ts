type RecordValue = Record<string, unknown>;

const count = (value: unknown): number | null => {
  if (value === null || value === undefined || value === '' || typeof value === 'boolean') return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? Math.floor(parsed) : null;
};

const timestamp = (value: unknown): number | null => {
  if (typeof value !== 'string' || !value) return null;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : null;
};

export const applyWorkflowMetrics = (
  item: RecordValue,
  source: RecordValue,
  sourceType: string,
  eventTimestamp: unknown
): void => {
  const meta = source.meta && typeof source.meta === 'object' ? source.meta as RecordValue : {};
  const time = timestamp(eventTimestamp);
  if (sourceType === 'tool_call' && time !== null) {
    item.toolStartedAtMs = time;
  }
  // Copy bounded runtime metadata before serializing the potentially huge business payload.
  // Never search tool arguments/results for fields that merely happen to share metric names.
  if (sourceType === 'tool_call' || sourceType === 'tool_result') {
    item.runtimeMetrics = true;
    if (Object.prototype.hasOwnProperty.call(source, 'request_context_tokens')) {
      item.request_context_tokens = count(source.request_context_tokens);
    }
    const usage = source.request_usage as RecordValue | undefined;
    if (usage) item.request_consumed_tokens = count(usage.total_tokens ?? usage.total);
  }
  if (sourceType === 'tool_result') {
    const duration = count(meta.duration_ms ?? source.duration_ms);
    if (duration !== null) {
      item.duration_ms = duration;
      item.durationSource = 'runtime';
    }
    finishWorkflowMetrics(item, eventTimestamp);
  }
};

export const finishWorkflowMetrics = (item: RecordValue, eventTimestamp: unknown): void => {
  const time = timestamp(eventTimestamp);
  const start = count(item.toolStartedAtMs);
  if (time === null || start === null || time < start) return;
  if (count(item.toolFinishedAtMs) === null) item.toolFinishedAtMs = time;
  if (count(item.duration_ms) === null) {
    item.duration_ms = time - start;
    item.durationSource = 'event_interval';
  }
};
