import { selectChatRuntimeMessage, selectVisibleMessageProjections } from '@/realtime/chat/chatRuntimeSelectors';
import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeProjection
} from '@/realtime/chat/chatRuntimeTypes';
import { chatDebugLog } from '@/utils/chatDebug';

type TextSnapshot = {
  content: string;
  reasoning: string;
};

type StreamTextStats = {
  contentDeltaChars: number;
  reasoningDeltaChars: number;
  finalContentChars: number;
  finalReasoningChars: number;
};

type ProjectedAssistantTextState = {
  messageId: string;
  userTurnId: string;
  modelTurnId: string;
  status: string;
  contentLength: number;
  reasoningLength: number;
};

export type TerminalSnapshotSmoothingPlan = {
  sessionId: string;
  requestId: string;
  terminalEventId: string;
  userTurnId: string;
  modelTurnId: string;
  assistantMessageId: string;
  finalContent: string;
  finalReasoning: string;
  currentContent: string;
  tail: string;
  tailChars: string[];
  chunkCount: number;
  chunkDelayMs: number;
  lastContentEventGapMs: number | null;
};

export type TerminalSnapshotSmoothingAnalysis = {
  plan: TerminalSnapshotSmoothingPlan | null;
  debug: Record<string, unknown>;
};

type AnalyzeTerminalSnapshotSmoothingOptions = {
  projection: ChatRuntimeProjection | null | undefined;
  sessionId: string;
  payload: unknown;
  approvalPayload?: unknown;
  requestId?: string;
  eventId?: string | number | null;
  userTurnId: string;
  modelTurnId: string;
  assistantMessageId: string;
  lastContentEventAt?: number;
};

type RunTerminalSnapshotSmoothingOptions = {
  plan: TerminalSnapshotSmoothingPlan;
  applyDelta: (delta: string, chunkIndex: number) => void;
  shouldContinue?: () => boolean;
};

const MIN_TERMINAL_TAIL_SMOOTH_CHARS = 48;
const MAX_TERMINAL_TAIL_SMOOTH_CHARS = 20_000;
const TARGET_TERMINAL_TAIL_CHUNK_CHARS = 18;
const MAX_TERMINAL_TAIL_CHUNKS = 30;
const MIN_TERMINAL_TAIL_CHUNKS = 2;
const MIN_TERMINAL_TAIL_CHUNK_DELAY_MS = 12;
const MAX_TERMINAL_TAIL_CHUNK_DELAY_MS = 22;
const MAX_TERMINAL_TAIL_SMOOTH_DURATION_MS = 540;

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};

const readData = (payload: Record<string, unknown>): Record<string, unknown> => {
  const nested = asRecord(payload.data);
  return Object.keys(nested).length > 0 ? nested : payload;
};

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'string' && value) return value;
    if (value !== null && value !== undefined && typeof value !== 'object') {
      const text = String(value);
      if (text) return text;
    }
  }
  return '';
};

const readSegments = (source: Record<string, unknown>): Record<string, unknown>[] => {
  if (Array.isArray(source.segments)) {
    return source.segments.map(asRecord).filter((item) => Object.keys(item).length > 0);
  }
  const nested = asRecord(source.data);
  if (Array.isArray(nested.segments)) {
    return nested.segments.map(asRecord).filter((item) => Object.keys(item).length > 0);
  }
  return [];
};

const extractSegmentText = (
  payload: Record<string, unknown>,
  data: Record<string, unknown>
): TextSnapshot => {
  const segments = readSegments(data).concat(readSegments(payload));
  if (segments.length === 0) return { content: '', reasoning: '' };
  let content = '';
  let reasoning = '';
  segments.forEach((segment) => {
    content += firstText(segment.content, segment.delta, segment.message);
    reasoning += firstText(
      segment.reasoning,
      segment.reasoning_delta,
      segment.reasoningDelta,
      segment.think_content,
      segment.thinkContent,
      segment.think_delta,
      segment.thinkDelta
    );
  });
  return { content, reasoning };
};

export const extractTerminalSnapshotText = (
  payloadValue: unknown,
  approvalPayloadValue?: unknown
): TextSnapshot => {
  const payload = asRecord(payloadValue);
  const data = readData(payload);
  const approvalPayload = asRecord(approvalPayloadValue);
  const segmentText = extractSegmentText(payload, data);
  return {
    content: firstText(
      segmentText.content,
      data.answer,
      approvalPayload.answer,
      payload.answer,
      data.content,
      approvalPayload.content,
      payload.content,
      data.message,
      approvalPayload.message,
      payload.message
    ),
    reasoning: firstText(
      segmentText.reasoning,
      data.reasoning,
      approvalPayload.reasoning,
      payload.reasoning,
      data.reasoning_content,
      data.reasoningContent,
      approvalPayload.reasoning_content,
      approvalPayload.reasoningContent,
      payload.reasoning_content,
      payload.reasoningContent,
      data.think_content,
      data.thinkContent,
      approvalPayload.think_content,
      approvalPayload.thinkContent,
      payload.think_content,
      payload.thinkContent
    )
  };
};

const extractDeltaText = (
  payloadValue: unknown,
  approvalPayloadValue?: unknown
): TextSnapshot => {
  const payload = asRecord(payloadValue);
  const data = readData(payload);
  const approvalPayload = asRecord(approvalPayloadValue);
  const segmentText = extractSegmentText(payload, data);
  return {
    content: firstText(
      segmentText.content,
      data.delta,
      approvalPayload.delta,
      payload.delta,
      data.content_delta,
      data.contentDelta,
      approvalPayload.content_delta,
      approvalPayload.contentDelta,
      payload.content_delta,
      payload.contentDelta,
      data.content,
      approvalPayload.content,
      payload.content,
      data.message,
      approvalPayload.message,
      payload.message
    ),
    reasoning: firstText(
      segmentText.reasoning,
      data.reasoning_delta,
      data.reasoningDelta,
      approvalPayload.reasoning_delta,
      approvalPayload.reasoningDelta,
      payload.reasoning_delta,
      payload.reasoningDelta,
      data.think_delta,
      data.thinkDelta,
      approvalPayload.think_delta,
      approvalPayload.thinkDelta,
      payload.think_delta,
      payload.thinkDelta
    )
  };
};

export const resolveStreamEventTextStats = (
  eventType: unknown,
  payload: unknown,
  approvalPayload?: unknown
): StreamTextStats => {
  const normalizedEventType = String(eventType || '').trim().toLowerCase();
  if (normalizedEventType === 'llm_output' || normalizedEventType === 'final') {
    const finalText = extractTerminalSnapshotText(payload, approvalPayload);
    return {
      contentDeltaChars: 0,
      reasoningDeltaChars: 0,
      finalContentChars: finalText.content.length,
      finalReasoningChars: finalText.reasoning.length
    };
  }
  const deltaText = extractDeltaText(payload, approvalPayload);
  return {
    contentDeltaChars: deltaText.content.length,
    reasoningDeltaChars: deltaText.reasoning.length,
    finalContentChars: 0,
    finalReasoningChars: 0
  };
};

const looksLikePlainTerminalText = (source: string): boolean => {
  if (!source) return false;
  if (source.length > 12_000) return false;
  if (source.includes('```') || source.includes('~~~')) return false;
  if (source.includes('|') && /\n\s*\|?[\s:-]+\|/.test(source)) return false;
  if (/!\[[^\]]*]\(|\[[^\]]+]\(|<https?:\/\//i.test(source)) return false;
  if (/^\s{0,3}(#{1,6}\s|[-*+]\s|\d+\.\s|>\s)/m.test(source)) return false;
  if (/(\*|_|~~|`|\$\$|\\\(|\\\[)/.test(source)) return false;
  return true;
};

const selectExpectedAssistantMessage = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: string,
  userTurnId: string,
  modelTurnId: string,
  assistantMessageId: string
): ChatRuntimeMessageProjection | null => {
  const direct = assistantMessageId
    ? selectChatRuntimeMessage(projection, sessionId, assistantMessageId)
    : null;
  if (direct?.role === 'assistant') return direct;
  const visible = selectVisibleMessageProjections(projection, sessionId);
  for (let index = visible.length - 1; index >= 0; index -= 1) {
    const message = visible[index];
    if (message?.role !== 'assistant') continue;
    if (modelTurnId && message.modelTurnId === modelTurnId) return message;
    if (userTurnId && message.userTurnId === userTurnId) return message;
  }
  return null;
};

export const resolveProjectedAssistantTextState = (
  projection: ChatRuntimeProjection | null | undefined,
  sessionId: string,
  userTurnId: string,
  modelTurnId: string,
  assistantMessageId: string
): ProjectedAssistantTextState | null => {
  const message = selectExpectedAssistantMessage(
    projection,
    sessionId,
    userTurnId,
    modelTurnId,
    assistantMessageId
  );
  if (!message) return null;
  return {
    messageId: message.id,
    userTurnId: message.userTurnId,
    modelTurnId: message.modelTurnId,
    status: message.status,
    contentLength: String(message.content || '').length,
    reasoningLength: String(message.reasoning || '').length
  };
};

const resolveChunkCount = (tailLength: number): number => {
  if (tailLength <= 0) return 0;
  return Math.max(
    MIN_TERMINAL_TAIL_CHUNKS,
    Math.min(MAX_TERMINAL_TAIL_CHUNKS, Math.ceil(tailLength / TARGET_TERMINAL_TAIL_CHUNK_CHARS))
  );
};

const resolveChunkDelayMs = (chunkCount: number): number => {
  if (chunkCount <= 1) return 0;
  return Math.max(
    MIN_TERMINAL_TAIL_CHUNK_DELAY_MS,
    Math.min(
      MAX_TERMINAL_TAIL_CHUNK_DELAY_MS,
      Math.floor(MAX_TERMINAL_TAIL_SMOOTH_DURATION_MS / chunkCount)
    )
  );
};

export const analyzeTerminalSnapshotSmoothing = (
  options: AnalyzeTerminalSnapshotSmoothingOptions
): TerminalSnapshotSmoothingAnalysis => {
  const sessionId = String(options.sessionId || '').trim();
  const userTurnId = String(options.userTurnId || '').trim();
  const modelTurnId = String(options.modelTurnId || '').trim();
  const assistantMessageId = String(options.assistantMessageId || '').trim();
  const requestId = String(options.requestId || '').trim();
  const terminalEventId = String(options.eventId ?? '').trim();
  const snapshot = extractTerminalSnapshotText(options.payload, options.approvalPayload);
  const message = selectExpectedAssistantMessage(
    options.projection,
    sessionId,
    userTurnId,
    modelTurnId,
    assistantMessageId
  );
  const currentContent = String(message?.content || '');
  const lastContentEventGapMs =
    Number(options.lastContentEventAt || 0) > 0
      ? Math.max(0, Date.now() - Number(options.lastContentEventAt || 0))
      : null;
  let reason = '';
  let tail = '';
  if (!sessionId || !snapshot.content) {
    reason = 'missing_snapshot';
  } else if (!message) {
    reason = 'missing_projected_message';
  } else if (snapshot.content.length <= currentContent.length) {
    reason = 'not_longer_than_projection';
  } else if (currentContent && !snapshot.content.startsWith(currentContent)) {
    reason = 'not_prefix';
  } else {
    tail = snapshot.content.slice(currentContent.length);
    if (tail.length < MIN_TERMINAL_TAIL_SMOOTH_CHARS) {
      reason = 'tail_too_short';
    } else if (tail.length > MAX_TERMINAL_TAIL_SMOOTH_CHARS) {
      reason = 'tail_too_long';
    } else if (!looksLikePlainTerminalText(snapshot.content)) {
      reason = 'not_plain_text';
    }
  }

  const shouldSmooth = !reason;
  const tailChars = shouldSmooth ? Array.from(tail) : [];
  const chunkCount = resolveChunkCount(tailChars.length);
  const plan = shouldSmooth
    ? {
        sessionId,
        requestId,
        terminalEventId,
        userTurnId,
        modelTurnId,
        assistantMessageId: message?.id || assistantMessageId,
        finalContent: snapshot.content,
        finalReasoning: snapshot.reasoning,
        currentContent,
        tail,
        tailChars,
        chunkCount,
        chunkDelayMs: resolveChunkDelayMs(chunkCount),
        lastContentEventGapMs
      }
    : null;
  return {
    plan,
    debug: {
      sessionId,
      requestId,
      terminalEventId: terminalEventId || null,
      userTurnId,
      modelTurnId,
      assistantMessageId: message?.id || assistantMessageId,
      projectedStatus: message?.status || '',
      projectedContentChars: currentContent.length,
      finalContentChars: snapshot.content.length,
      finalReasoningChars: snapshot.reasoning.length,
      tailChars: Math.max(0, snapshot.content.length - currentContent.length),
      lastContentEventGapMs,
      shouldSmooth,
      smoothReason: shouldSmooth ? 'terminal_tail_prefix' : reason
    }
  };
};

export const buildTerminalSnapshotDeltaPayload = (
  plan: TerminalSnapshotSmoothingPlan,
  delta: string,
  chunkIndex: number
): Record<string, unknown> => ({
  session_id: plan.sessionId,
  request_id: plan.requestId || undefined,
  user_turn_id: plan.userTurnId,
  model_turn_id: plan.modelTurnId,
  message_id: plan.assistantMessageId,
  assistant_message_id: plan.assistantMessageId,
  delta,
  content_delta: delta,
  synthetic_terminal_snapshot_delta: true,
  terminal_snapshot_event_id: plan.terminalEventId || undefined,
  event_id: `synthetic-terminal-tail:${plan.requestId || 'request'}:${plan.terminalEventId || 'terminal'}:${chunkIndex}`
});

const waitMs = (delayMs: number): Promise<void> =>
  new Promise((resolve) => {
    globalThis.setTimeout(resolve, Math.max(0, delayMs));
  });

export const runTerminalSnapshotSmoothing = async (
  options: RunTerminalSnapshotSmoothingOptions
): Promise<{ completed: boolean; appliedChars: number; chunkCount: number }> => {
  const { plan } = options;
  const shouldContinue = options.shouldContinue || (() => true);
  const chunkSize = Math.max(1, Math.ceil(plan.tailChars.length / Math.max(1, plan.chunkCount)));
  let cursor = 0;
  let chunkIndex = 0;
  while (cursor < plan.tailChars.length) {
    if (!shouldContinue()) {
      chatDebugLog('chat.stream.perf', 'terminal-tail-smoothing-interrupted', {
        sessionId: plan.sessionId,
        requestId: plan.requestId,
        terminalEventId: plan.terminalEventId || null,
        appliedChars: cursor,
        tailChars: plan.tailChars.length,
        chunkIndex
      });
      return { completed: false, appliedChars: cursor, chunkCount: chunkIndex };
    }
    const nextCursor = Math.min(plan.tailChars.length, cursor + chunkSize);
    const delta = plan.tailChars.slice(cursor, nextCursor).join('');
    chunkIndex += 1;
    options.applyDelta(delta, chunkIndex);
    cursor = nextCursor;
    if (cursor < plan.tailChars.length) {
      await waitMs(plan.chunkDelayMs);
    }
  }
  return { completed: true, appliedChars: cursor, chunkCount: chunkIndex };
};
