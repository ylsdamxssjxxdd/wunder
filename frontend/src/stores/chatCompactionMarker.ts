import { chatDebugLog } from '../utils/chatDebug';

type ChatMessage = Record<string, any>;
type WorkflowItem = Record<string, unknown>;

const CONTEXT_CN = '\u4e0a\u4e0b\u6587';
const COMPACTION_CN = '\u538b\u7f29';

const normalizeText = (value: unknown): string => String(value ?? '').trim().toLowerCase();

const asObject = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const parseDetailObject = (value: unknown): Record<string, unknown> | null => {
  const direct = asObject(value);
  if (direct) return direct;
  const text = String(value ?? '').trim();
  if (!text) return null;
  try {
    const parsed = JSON.parse(text);
    return asObject(parsed);
  } catch {
    return null;
  }
};

const resolveTimestampMs = (value: unknown): number | null => {
  const text = String(value ?? '').trim();
  if (!text) return null;
  const millis = Date.parse(text);
  return Number.isFinite(millis) ? millis : null;
};

const hasTextContent = (value: unknown): boolean => String(value ?? '').trim().length > 0;

const isStreamingAssistantMessage = (message: ChatMessage | null | undefined): boolean =>
  Boolean(
    message?.workflowStreaming ||
      message?.reasoningStreaming ||
      message?.stream_incomplete
  );

const hasManualCompactionMarkerFlag = (message: ChatMessage | null | undefined): boolean =>
  Boolean(message?.manual_compaction_marker === true || message?.manualCompactionMarker === true);

const isManualCompactionWorkflowItem = (value: unknown): boolean => {
  const item = asObject(value);
  if (!item) return false;
  const detail = parseDetailObject(item.detail ?? item.data ?? item.payload);
  const triggerMode = normalizeText(detail?.trigger_mode ?? detail?.triggerMode);
  if (triggerMode === 'manual') {
    return true;
  }
  const workflowRef = normalizeText(item.toolCallId ?? item.tool_call_id ?? item.callId ?? item.call_id);
  return workflowRef.startsWith('compaction:manual:');
};

const isManualCompactionMessage = (message: ChatMessage | null | undefined): boolean => {
  const items = Array.isArray(message?.workflowItems) ? message.workflowItems : [];
  return items.some((item) => isManualCompactionWorkflowItem(item));
};

const hasPlanSteps = (plan: unknown): boolean =>
  Array.isArray((plan as { steps?: unknown[] } | null)?.steps) &&
  ((plan as { steps?: unknown[] } | null)?.steps?.length || 0) > 0;

const pickString = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value !== 'string') continue;
    const trimmed = value.trim();
    if (trimmed) return trimmed;
  }
  return '';
};

const isCompactionEventType = (value: unknown): boolean => {
  const text = normalizeText(value);
  return text === 'compaction' || text === 'compaction_progress';
};

const isCompactionToolName = (value: unknown): boolean => {
  const text = normalizeText(value);
  if (!text) return false;
  if (text === 'context_compaction' || text === 'context_compact' || text === 'compaction') {
    return true;
  }
  if (text === `${CONTEXT_CN}${COMPACTION_CN}`) {
    return true;
  }
  if (text.includes('context') && text.includes('compact')) {
    return true;
  }
  return text.includes(CONTEXT_CN) && text.includes(COMPACTION_CN);
};

const isCompactionWorkflowItem = (value: unknown): boolean => {
  const item = asObject(value);
  if (!item) return false;
  if (normalizeText(item.eventType ?? item.event) === 'compaction_notice') return true;
  if (isCompactionEventType(item.eventType ?? item.event)) return true;
  return isCompactionToolName(item.toolName ?? item.tool ?? item.name);
};

const isCompactionOnlyWorkflowItems = (items: unknown): boolean => {
  if (!Array.isArray(items) || items.length === 0) return false;
  let hasCompaction = false;
  for (const item of items) {
    if (isCompactionWorkflowItem(item)) {
      hasCompaction = true;
      continue;
    }
    return false;
  }
  return hasCompaction;
};

export const isCompactionMarkerAssistantMessage = (message: ChatMessage | null | undefined): boolean => {
  if (!message || message.role !== 'assistant') return false;
  if (hasTextContent(message.content) || hasTextContent(message.reasoning)) return false;
  if (hasPlanSteps(message.plan)) return false;
  const panelStatus = normalizeText((message.questionPanel as Record<string, unknown> | null)?.status);
  if (panelStatus === 'pending') return false;
  if (hasManualCompactionMarkerFlag(message)) return true;
  if (!isCompactionOnlyWorkflowItems(message.workflowItems)) return false;
  if (!isStreamingAssistantMessage(message)) return true;
  return isManualCompactionMessage(message);
};

const resolveWorkflowCallRef = (message: ChatMessage): string => {
  const items = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  for (let cursor = items.length - 1; cursor >= 0; cursor -= 1) {
    const item = asObject(items[cursor]) as WorkflowItem | null;
    if (!item) continue;
    const callId = normalizeText(item.toolCallId ?? item.tool_call_id ?? item.callId ?? item.call_id);
    if (callId) return callId;
  }
  return '';
};

const resolveWorkflowShape = (message: ChatMessage): string => {
  const items = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  if (items.length === 0) return '';
  const first = asObject(items[0]);
  const last = asObject(items[items.length - 1]);
  const firstType = normalizeText(first?.eventType ?? first?.event);
  const lastType = normalizeText(last?.eventType ?? last?.event);
  const lastStatus = normalizeText(last?.status);
  const detail = parseDetailObject(last?.detail ?? last?.data ?? last?.payload);
  const detailStatus = normalizeText(detail?.status);
  const detailStage = normalizeText(detail?.stage);
  return [items.length, firstType, lastType, lastStatus, detailStatus, detailStage]
    .map((part) => String(part))
    .join(':');
};

const resolveCompactionIdentity = (message: ChatMessage): string => {
  const items = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  for (let cursor = items.length - 1; cursor >= 0; cursor -= 1) {
    const item = asObject(items[cursor]) as WorkflowItem | null;
    if (!item || !isCompactionWorkflowItem(item)) continue;
    const detail = parseDetailObject(item.detail ?? item.data ?? item.payload);
    if (!detail) continue;
    const before = String(
      detail.projected_request_tokens ??
        detail.total_tokens ??
        detail.context_tokens ??
        detail.context_guard_tokens_before ??
        ''
    ).trim();
    const after = String(
      detail.projected_request_tokens_after ??
        detail.total_tokens_after ??
        detail.context_tokens_after ??
        detail.context_guard_tokens_after ??
        detail.final_context_tokens ??
        ''
    ).trim();
    const errorCode = normalizeText(detail.error_code ?? detail.errorCode);
    const detailStatus = normalizeText(detail.status);
    const summaryText = pickString(
      detail.summary_text,
      detail.summaryText,
      detail.summary_model_output,
      detail.summaryModelOutput,
      detail.compaction_summary_text,
      detail.compactionSummaryText
    )
      .replace(/\s+/g, ' ')
      .slice(0, 120)
      .toLowerCase();
    return [detailStatus, before, after, errorCode, summaryText].join('|');
  }
  return '';
};

const resolveCompactionMarkerSignature = (message: ChatMessage): string => {
  const createdAt = String(message.created_at ?? '').trim();
  const shape = resolveWorkflowShape(message);
  const identity = resolveCompactionIdentity(message);
  if (identity) {
    return [shape, identity].join('|');
  }
  const callRef = resolveWorkflowCallRef(message);
  return [createdAt, callRef, shape].join('|');
};

const cloneCompactionMarker = (message: ChatMessage): ChatMessage => ({
  ...message,
  workflowItems: Array.isArray(message.workflowItems)
    ? message.workflowItems.map((item) => (asObject(item) ? { ...(item as Record<string, unknown>) } : item))
    : []
});

const resolveInsertIndexByTimestamp = (messages: ChatMessage[], markerTime: number): number => {
  for (let index = 0; index < messages.length; index += 1) {
    const current = messages[index];
    const currentTime = resolveTimestampMs(current?.created_at);
    if (currentTime === null) continue;
    if (currentTime > markerTime) {
      return index;
    }
  }
  return messages.length;
};

const resolveStreamRound = (value: unknown): number | null => {
  if (value === null || value === undefined || value === '') return null;
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return null;
  const normalized = Math.trunc(parsed);
  return normalized > 0 ? normalized : null;
};

const resolveManualCompactionRound = (message: ChatMessage | null | undefined): number | null => {
  const directRound = resolveStreamRound(message?.stream_round ?? message?.streamRound);
  if (directRound !== null) {
    return directRound;
  }
  const items = Array.isArray(message?.workflowItems) ? message.workflowItems : [];
  for (let cursor = items.length - 1; cursor >= 0; cursor -= 1) {
    const item = asObject(items[cursor]);
    if (!item || !isManualCompactionWorkflowItem(item)) continue;
    const detail = parseDetailObject(item.detail ?? item.data ?? item.payload);
    const round = resolveStreamRound(detail?.user_round ?? detail?.userRound ?? detail?.round);
    if (round !== null) {
      return round;
    }
  }
  return null;
};

const isRunningManualCompactionMarker = (message: ChatMessage | null | undefined): boolean =>
  Boolean(
    message &&
      hasManualCompactionMarkerFlag(message) &&
      isManualCompactionMessage(message) &&
      isStreamingAssistantMessage(message)
  );

const isTerminalManualCompactionMarker = (message: ChatMessage | null | undefined): boolean =>
  Boolean(
    message &&
      hasManualCompactionMarkerFlag(message) &&
      isManualCompactionMessage(message) &&
      !isStreamingAssistantMessage(message)
  );

const isManualCompactionConflict = (
  remoteMessage: ChatMessage,
  runningMarker: ChatMessage
): boolean => {
  if (!isTerminalManualCompactionMarker(remoteMessage)) return false;
  if (!isRunningManualCompactionMarker(runningMarker)) return false;
  const remoteCallRef = resolveWorkflowCallRef(remoteMessage);
  const runningCallRef = resolveWorkflowCallRef(runningMarker);
  if (remoteCallRef && runningCallRef && remoteCallRef === runningCallRef) {
    return true;
  }
  const remoteRound = resolveManualCompactionRound(remoteMessage);
  const runningRound = resolveManualCompactionRound(runningMarker);
  const remoteTime = resolveTimestampMs(remoteMessage.created_at);
  const runningTime = resolveTimestampMs(runningMarker.created_at);
  if (remoteRound !== null && runningRound !== null && remoteRound === runningRound) {
    if (remoteTime === null || runningTime === null) {
      return true;
    }
    return Math.abs(remoteTime - runningTime) <= 10_000;
  }
  if (remoteTime !== null && runningTime !== null) {
    return Math.abs(remoteTime - runningTime) <= 1_500;
  }
  return false;
};

export const mergeCompactionMarkersIntoMessages = (
  remoteMessages: ChatMessage[] | null | undefined,
  cachedMessages: ChatMessage[] | null | undefined
): ChatMessage[] => {
  const baseMessages = Array.isArray(remoteMessages) ? remoteMessages : [];
  if (!Array.isArray(cachedMessages) || cachedMessages.length === 0) {
    return baseMessages;
  }
  const cachedMarkers = cachedMessages
    .filter((message) => isCompactionMarkerAssistantMessage(message))
    .map((message, index) => ({
      index,
      time: resolveTimestampMs(message.created_at),
      signature: resolveCompactionMarkerSignature(message),
      message: cloneCompactionMarker(message)
    }));
  if (!cachedMarkers.length) {
    return baseMessages;
  }
  const remoteTerminalManualMarkers = baseMessages.filter((message) =>
    isTerminalManualCompactionMarker(message)
  );
  const suppressed: string[] = [];
  const result = [...baseMessages];
  const existingSignatures = new Set(
    result
      .filter((message) => isCompactionMarkerAssistantMessage(message))
      .map((message) => resolveCompactionMarkerSignature(message))
  );
  let changed = false;
  const inserted: string[] = [];
  const skipped: string[] = [];
  const sortableMarkers = [...cachedMarkers].sort((left, right) => {
    if (left.time !== null && right.time !== null && left.time !== right.time) {
      return left.time - right.time;
    }
    if (left.time !== null && right.time === null) return -1;
    if (left.time === null && right.time !== null) return 1;
    return left.index - right.index;
  });
  sortableMarkers.forEach((entry) => {
    const signature = entry.signature;
    const conflictsWithRemoteTerminal =
      isRunningManualCompactionMarker(entry.message)
      && remoteTerminalManualMarkers.some((message) =>
        isManualCompactionConflict(message, entry.message)
      );
    if (conflictsWithRemoteTerminal) {
      suppressed.push(signature);
      return;
    }
    if (existingSignatures.has(signature)) {
      skipped.push(signature);
      return;
    }
    const insertIndex =
      entry.time === null ? result.length : resolveInsertIndexByTimestamp(result, entry.time);
    result.splice(insertIndex, 0, entry.message);
    existingSignatures.add(signature);
    inserted.push(signature);
    changed = true;
  });
  if (cachedMarkers.length > 0) {
    chatDebugLog('chat.compaction.marker', 'merge', {
      cachedMarkerCount: cachedMarkers.length,
      remoteMessageCount: baseMessages.length,
      remoteMarkerCount: Array.from(existingSignatures).length - inserted.length,
      suppressed,
      inserted,
      skipped,
      changed
    });
  }
  return changed ? result : baseMessages;
};
