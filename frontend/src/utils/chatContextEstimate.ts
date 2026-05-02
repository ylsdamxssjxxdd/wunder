type UnknownRecord = Record<string, unknown>;

const MODEL_PAYLOAD_SCORE_KEYS = [
  'messages',
  'tools',
  'tool_choice',
  'response_format',
  'chat_template_kwargs',
  'system',
  'developer',
  'model'
] as const;

const asRecord = (value: unknown): UnknownRecord | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as UnknownRecord;
};

const estimateSerializedPayloadTokens = (value: unknown): number | null => {
  const record = asRecord(value);
  if (!record) return null;
  const seen = new WeakSet<object>();
  let serialized = '';
  try {
    serialized = JSON.stringify(record, (_key, item) => {
      if (item === undefined || typeof item === 'function' || typeof item === 'symbol') {
        return undefined;
      }
      if (item && typeof item === 'object') {
        if (seen.has(item)) return undefined;
        seen.add(item);
      }
      return item;
    });
  } catch {
    return null;
  }
  if (!serialized) return null;
  const messageCount = Array.isArray(record.messages) ? record.messages.length : 0;
  const toolCount = Array.isArray(record.tools) ? record.tools.length : 0;
  const baseTokens = estimateChatTextTokens(serialized);
  const framingTokens = 8 + messageCount * 4 + toolCount * 3;
  return Math.max(1, Math.round((baseTokens + framingTokens) * 1.12));
};

const resolveSummaryCount = (value: unknown): number => {
  if (Array.isArray(value)) return value.length;
  const record = asRecord(value);
  if (!record) return 0;
  const parsed = Number.parseInt(String(record.count ?? record.length ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
};

const estimatePayloadSummaryTokens = (value: unknown): number | null => {
  const summary = asRecord(value);
  if (!summary) return null;
  const serializedEstimate = estimateSerializedPayloadTokens(summary);
  const messageCount = resolveSummaryCount(summary.messages ?? summary.input);
  const toolCount = resolveSummaryCount(summary.tools ?? summary.functions);
  const textLength = Number.parseInt(
    String(
      summary.string_length ??
        summary.stringLength ??
        summary.payload_length ??
        summary.payloadLength ??
        summary.length ??
        ''
    ),
    10
  );
  const lengthEstimate =
    Number.isFinite(textLength) && textLength > 0
      ? Math.round((textLength / 4 + messageCount * 4 + toolCount * 3) * 1.12)
      : null;
  const structuralEstimate =
    messageCount > 0 || toolCount > 0
      ? Math.round((messageCount * 750 + toolCount * 380 + 8) * 1.12)
      : null;
  const estimate = Math.max(
    serializedEstimate ?? 0,
    lengthEstimate ?? 0,
    structuralEstimate ?? 0
  );
  return estimate > 0 ? estimate : null;
};

const estimateStructuredTextTokens = (value: unknown, depth = 0): number => {
  if (value === null || value === undefined || depth > 4) return 0;
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return estimateChatTextTokens(String(value));
  }
  if (Array.isArray(value)) {
    return value.reduce((total, item) => total + estimateStructuredTextTokens(item, depth + 1), 0);
  }
  const record = asRecord(value);
  if (!record) return 0;
  let total = 0;
  for (const key of ['text', 'content', 'input', 'output', 'reasoning', 'reasoning_content']) {
    total += estimateStructuredTextTokens(record[key], depth + 1);
  }
  return total;
};

const estimateRequestMessagesTokens = (messages: unknown): number | null => {
  if (!Array.isArray(messages) || messages.length === 0) return null;
  let total = 2;
  messages.forEach((message) => {
    const record = asRecord(message);
    if (!record) return;
    total += 4;
    total += estimateStructuredTextTokens(record.content ?? record.text ?? record.message);
    total += estimateStructuredTextTokens(record.name);
  });
  return total > 0 ? total : null;
};

const scoreModelPayloadCandidate = (value: unknown): number => {
  const record = asRecord(value);
  if (!record) return 0;
  let score = 0;
  MODEL_PAYLOAD_SCORE_KEYS.forEach((key) => {
    if (record[key] !== undefined) score += 1;
  });
  if (Array.isArray(record.messages)) score += 4;
  if (Array.isArray(record.tools)) score += 4;
  return score;
};

const collectModelPayloadCandidates = (
  value: unknown,
  output: UnknownRecord[],
  seen: Set<UnknownRecord>,
  depth = 0
): void => {
  if (depth > 4) return;
  const record = asRecord(value);
  if (!record || seen.has(record)) return;
  seen.add(record);
  if (scoreModelPayloadCandidate(record) > 0) {
    output.push(record);
  }
  const nestedKeys = [
    'payload',
    'request',
    'data',
    'model_request',
    'modelRequest',
    'request_payload',
    'requestPayload'
  ];
  nestedKeys.forEach((key) => collectModelPayloadCandidates(record[key], output, seen, depth + 1));
};

const resolveBestModelPayload = (source: unknown): UnknownRecord | null => {
  const candidates: UnknownRecord[] = [];
  collectModelPayloadCandidates(source, candidates, new Set());
  if (candidates.length === 0) return null;
  return candidates.reduce((best, candidate) =>
    scoreModelPayloadCandidate(candidate) > scoreModelPayloadCandidate(best) ? candidate : best
  );
};

export const estimateChatTextTokens = (text: unknown): number => {
  if (!text) return 0;
  const source = String(text);
  let asciiVisible = 0;
  let cjkCount = 0;
  let otherCount = 0;
  for (const char of source) {
    if (!char || /\s/.test(char)) continue;
    const code = char.charCodeAt(0);
    if (code <= 0x7f) {
      asciiVisible += 1;
      continue;
    }
    if (
      (code >= 0x4e00 && code <= 0x9fff) ||
      (code >= 0x3400 && code <= 0x4dbf) ||
      (code >= 0xf900 && code <= 0xfaff)
    ) {
      cjkCount += 1;
      continue;
    }
    otherCount += 1;
  }
  const estimated = cjkCount + asciiVisible / 4 + otherCount * 0.75;
  return Math.max(0, Math.round(estimated));
};

export const estimateRequestContextTokens = (source: unknown): number | null => {
  const payload = resolveBestModelPayload(source);
  const sourceRecord = asRecord(source);
  const summaryEstimate =
    estimatePayloadSummaryTokens(sourceRecord?.payload_summary ?? sourceRecord?.payloadSummary) ??
    estimatePayloadSummaryTokens(
      asRecord(sourceRecord?.request)?.payload_summary ??
        asRecord(sourceRecord?.request)?.payloadSummary
    ) ??
    estimatePayloadSummaryTokens(
      asRecord(sourceRecord?.data)?.payload_summary ??
        asRecord(sourceRecord?.data)?.payloadSummary
    );
  if (!payload) return summaryEstimate;
  const messageEstimate = estimateRequestMessagesTokens(payload.messages);
  const serializedEstimate = estimateSerializedPayloadTokens(payload);
  const estimate = Math.max(messageEstimate ?? 0, serializedEstimate ?? 0, summaryEstimate ?? 0);
  return estimate > 0 ? estimate : null;
};
