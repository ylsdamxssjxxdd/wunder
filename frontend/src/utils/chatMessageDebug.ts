import { resolveChatRuntimeRenderableKey, resolveStableChatRuntimeMessageId } from '@/realtime/chat/chatRuntimeMessageKeys';

type ChatMessageLike = Record<string, unknown>;

export type ChatMessageDebugSnapshot = {
  index: number;
  role: string;
  key: string;
  messageId: string | null;
  clientMessageId: string | null;
  userTurnId: string | null;
  modelTurnId: string | null;
  streamEventId: number | null;
  streamRound: number | null;
  status: string | null;
  contentLength: number;
  reasoningLength: number;
  isGreeting: boolean;
  hiddenInternal: boolean;
  cancelled: boolean;
  failed: boolean;
  final: boolean;
  streamIncomplete: boolean;
  workflowStreaming: boolean;
  reasoningStreaming: boolean;
  resumeAvailable: boolean;
  slowClient: boolean;
  workflowCount: number;
  workflowActiveCount: number;
  subagentCount: number;
  subagentActiveCount: number;
  questionPanelStatus: string | null;
};

export type ChatMessageDebugListSummary = {
  count: number;
  assistantCount: number;
  userCount: number;
  greetingCount: number;
  hiddenInternalCount: number;
  cancelledCount: number;
  pendingAssistantCount: number;
  head: ChatMessageDebugSnapshot[];
  tail: ChatMessageDebugSnapshot[];
  signature: string;
};

const DEFAULT_LIST_LIMIT = 4;
const ACTIVE_WORKFLOW_STATUSES = new Set([
  'accepted',
  'cancelling',
  'in_progress',
  'inprogress',
  'loading',
  'pending',
  'processing',
  'queued',
  'running',
  'started',
  'streaming',
  'waiting'
]);
const ACTIVE_SUBAGENT_STATUSES = new Set([
  'accepted',
  'cancelling',
  'in_progress',
  'inprogress',
  'loading',
  'pending',
  'processing',
  'queued',
  'running',
  'started',
  'waiting'
]);

export const summarizeChatMessageDebugSnapshot = (
  message: ChatMessageLike | null | undefined,
  index = -1
): ChatMessageDebugSnapshot | null => {
  if (!message || typeof message !== 'object' || Array.isArray(message)) {
    return null;
  }
  const role = normalizeRole(message.role);
  if (!role) {
    return null;
  }
  const messageId = firstText(
    resolveStableChatRuntimeMessageId(message),
    message.message_id,
    message.messageId,
    message.id
  );
  const clientMessageId = firstText(message.client_message_id, message.clientMessageId);
  const userTurnId = firstText(message.user_turn_id, message.userTurnId);
  const modelTurnId = firstText(message.model_turn_id, message.modelTurnId);
  const streamEventId = normalizePositiveInteger(message.stream_event_id ?? message.streamEventId);
  const streamRound = normalizePositiveInteger(message.stream_round ?? message.streamRound);
  const status = normalizeStatus(
    firstText(message.runtime_status, message.status, message.state)
  );
  const workflowItems = normalizeRecordArray(message.workflowItems);
  const subagents = normalizeRecordArray(message.subagents);
  return {
    index: Number.isFinite(index) ? Math.max(0, Math.trunc(index)) : 0,
    role,
    key: resolveChatRuntimeRenderableKey(message, index),
    messageId: messageId || null,
    clientMessageId: clientMessageId || null,
    userTurnId: userTurnId || null,
    modelTurnId: modelTurnId || null,
    streamEventId,
    streamRound,
    status: status || null,
    contentLength: String(message.content || '').length,
    reasoningLength: String(message.reasoning || '').length,
    isGreeting: message.isGreeting === true,
    hiddenInternal: isHiddenInternalMessage(message),
    cancelled: normalizeFlag(message.cancelled) || status === 'cancelled' || status === 'canceled',
    failed: normalizeFlag(message.failed) || status === 'failed' || status === 'error',
    final:
      normalizeFlag(message.final) ||
      status === 'final' ||
      status === 'completed' ||
      status === 'complete' ||
      status === 'done',
    streamIncomplete: normalizeFlag(message.stream_incomplete),
    workflowStreaming: normalizeFlag(message.workflowStreaming) || normalizeFlag(message.workflow_streaming),
    reasoningStreaming: normalizeFlag(message.reasoningStreaming) || normalizeFlag(message.reasoning_streaming),
    resumeAvailable: normalizeFlag(message.resume_available) || normalizeFlag(message.resumeAvailable),
    slowClient: normalizeFlag(message.slow_client) || normalizeFlag(message.slowClient),
    workflowCount: workflowItems.length,
    workflowActiveCount: countActiveWorkflowItems(workflowItems),
    subagentCount: subagents.length,
    subagentActiveCount: countActiveSubagents(subagents),
    questionPanelStatus: resolveQuestionPanelStatus(message)
  };
};

export const summarizeChatMessageDebugList = (
  messages: ChatMessageLike[] | null | undefined,
  options: { limit?: number } = {}
): ChatMessageDebugListSummary => {
  const limit = Math.max(1, Number(options.limit) || DEFAULT_LIST_LIMIT);
  const snapshots = (Array.isArray(messages) ? messages : [])
    .map((message, index) => summarizeChatMessageDebugSnapshot(message, index))
    .filter((item): item is ChatMessageDebugSnapshot => Boolean(item));
  const head = snapshots.slice(0, limit);
  const tail = snapshots.length > limit ? snapshots.slice(-limit) : snapshots.slice();
  return {
    count: snapshots.length,
    assistantCount: snapshots.filter((item) => item.role === 'assistant').length,
    userCount: snapshots.filter((item) => item.role === 'user').length,
    greetingCount: snapshots.filter((item) => item.isGreeting).length,
    hiddenInternalCount: snapshots.filter((item) => item.hiddenInternal).length,
    cancelledCount: snapshots.filter((item) => item.cancelled).length,
    pendingAssistantCount: snapshots.filter((item) => isPendingAssistantSnapshot(item)).length,
    head,
    tail,
    signature: hashText(snapshots.map(buildMessageDebugSnapshotSignature).join('|'))
  };
};

export const buildMessageIdentityDebugSnapshot = summarizeChatMessageDebugSnapshot;

export const buildMessageIdentityDebugList = summarizeChatMessageDebugList;

const isPendingAssistantSnapshot = (snapshot: ChatMessageDebugSnapshot): boolean =>
  snapshot.role === 'assistant' &&
  (
    snapshot.streamIncomplete === true ||
    snapshot.workflowStreaming === true ||
    snapshot.reasoningStreaming === true ||
    snapshot.status === 'placeholder' ||
    snapshot.status === 'waiting_first_output' ||
    snapshot.status === 'streaming' ||
    snapshot.status === 'tooling'
  );

const buildMessageDebugSnapshotSignature = (snapshot: ChatMessageDebugSnapshot): string =>
  [
    snapshot.index,
    snapshot.role,
    snapshot.key,
    snapshot.messageId || '',
    snapshot.clientMessageId || '',
    snapshot.userTurnId || '',
    snapshot.modelTurnId || '',
    snapshot.streamEventId ?? '',
    snapshot.streamRound ?? '',
    snapshot.status || '',
    snapshot.contentLength,
    snapshot.reasoningLength,
    snapshot.isGreeting ? 1 : 0,
    snapshot.hiddenInternal ? 1 : 0,
    snapshot.cancelled ? 1 : 0,
    snapshot.failed ? 1 : 0,
    snapshot.final ? 1 : 0,
    snapshot.streamIncomplete ? 1 : 0,
    snapshot.workflowStreaming ? 1 : 0,
    snapshot.reasoningStreaming ? 1 : 0,
    snapshot.resumeAvailable ? 1 : 0,
    snapshot.slowClient ? 1 : 0,
    snapshot.workflowCount,
    snapshot.workflowActiveCount,
    snapshot.subagentCount,
    snapshot.subagentActiveCount,
    snapshot.questionPanelStatus || ''
  ].join('|');

const countActiveWorkflowItems = (items: ChatMessageLike[]): number =>
  items.reduce((count, item) => {
    const status = normalizeStatus(item?.status);
    return count + (ACTIVE_WORKFLOW_STATUSES.has(status) ? 1 : 0);
  }, 0);

const countActiveSubagents = (items: ChatMessageLike[]): number =>
  items.reduce((count, item) => {
    const agentState = isPlainRecord(item?.agent_state)
      ? item.agent_state
      : isPlainRecord(item?.agentState)
        ? item.agentState
        : {};
    const status = normalizeStatus(firstText(item?.status, agentState.status));
    if (ACTIVE_SUBAGENT_STATUSES.has(status)) {
      return count + 1;
    }
    return count;
  }, 0);

const isHiddenInternalMessage = (message: ChatMessageLike): boolean => {
  if (message.hiddenInternal === true || message.hidden === true) {
    return true;
  }
  const meta = isPlainRecord(message.meta) ? message.meta : {};
  const metaType = normalizeStatus(meta?.type);
  return Boolean(
    meta.hidden === true ||
    meta.internal_user === true ||
    metaType === 'model_context_internal'
  );
};

const resolveQuestionPanelStatus = (message: ChatMessageLike): string | null => {
  const panel = isPlainRecord(message.questionPanel) ? message.questionPanel : null;
  const status = normalizeStatus(panel?.status);
  return status || null;
};

const normalizeRecordArray = (value: unknown): ChatMessageLike[] =>
  Array.isArray(value)
    ? value.filter((item): item is ChatMessageLike => Boolean(item && typeof item === 'object' && !Array.isArray(item)))
    : [];

const isPlainRecord = (value: unknown): value is ChatMessageLike =>
  Boolean(value && typeof value === 'object' && !Array.isArray(value));

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    const text = normalizeText(value);
    if (text) return text;
  }
  return '';
};

const normalizeText = (value: unknown): string => String(value ?? '').trim();

const normalizeStatus = (value: unknown): string => normalizeText(value).toLowerCase();

const normalizeRole = (value: unknown): string => {
  const role = normalizeStatus(value);
  return role === 'user' || role === 'assistant' || role === 'system' ? role : '';
};

const normalizePositiveInteger = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase();
    if (!text) return false;
    return text !== 'false' && text !== '0' && text !== 'no';
  }
  return Boolean(value);
};

const hashText = (value: string): string => {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
};
