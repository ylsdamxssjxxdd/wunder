import {
  hasAssistantWaitingForCurrentOutput,
  isAssistantMessageRunning
} from './assistantMessageRuntime';
import { hasActiveSubagentItems } from './subagentRuntime';

type ChatMessage = Record<string, unknown>;
export type ThreadRuntimeStatus =
  | 'not_loaded'
  | 'idle'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'queued'
  | 'running'
  | 'waiting_approval'
  | 'waiting_user_input'
  | 'system_error';

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase();
    if (!text) return false;
    return text !== 'false' && text !== '0' && text !== 'no';
  }
  return Boolean(value);
};

const resolveLatestUserIndex = (messages: ChatMessage[]): number => {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (messages[index]?.role === 'user') {
      return index;
    }
  }
  return -1;
};

const ACTIVE_BLOCKING_TOOL_STATUSES = new Set([
  'loading',
  'pending',
  'queued',
  'running',
  'streaming',
  'waiting'
]);

const isBlockingSwarmWorkflowItem = (item: unknown): boolean => {
  if (!item || typeof item !== 'object' || Array.isArray(item)) return false;
  const record = item as Record<string, unknown>;
  const status = String(record.status || '').trim().toLowerCase();
  if (!ACTIVE_BLOCKING_TOOL_STATUSES.has(status)) return false;
  const eventType = String(
    record.eventType ?? record.event_type ?? record.event ?? record.sourceEventType ?? record.source_event_type ?? ''
  ).trim().toLowerCase();
  if (eventType !== 'tool_call' && eventType !== 'tool_call_started') return false;
  const identities = [
    record.toolFunctionName,
    record.tool_function_name,
    record.functionName,
    record.function_name,
    record.toolRuntimeName,
    record.tool_runtime_name,
    record.runtimeName,
    record.runtime_name,
    record.toolName,
    record.tool_name,
    record.tool,
    record.name,
    record.toolDisplayName,
    record.tool_display_name,
    record.displayName,
    record.display_name
  ].map((value) => String(value || '').trim().toLowerCase());
  return identities.some((identity) =>
    identity === 'agent_swarm' || identity.includes('@agent_swarm') || identity === '智能体蜂群'
  );
};

export const normalizeThreadRuntimeStatus = (value: unknown): ThreadRuntimeStatus => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'idle') return 'idle';
  if (normalized === 'completed' || normalized === 'complete' || normalized === 'done') return 'completed';
  if (normalized === 'failed' || normalized === 'error') return 'failed';
  if (normalized === 'cancelled' || normalized === 'canceled') return 'cancelled';
  if (normalized === 'queued' || normalized === 'pending' || normalized === 'waiting') return 'queued';
  if (normalized === 'running') return 'running';
  if (normalized === 'waiting_approval') return 'waiting_approval';
  if (normalized === 'waiting_user_input') return 'waiting_user_input';
  if (normalized === 'system_error') return 'system_error';
  return 'not_loaded';
};

export const isThreadRuntimeWaiting = (status: unknown): boolean => {
  const normalized = normalizeThreadRuntimeStatus(status);
  return normalized === 'waiting_approval' || normalized === 'waiting_user_input';
};

export const isThreadRuntimeBusy = (status: unknown): boolean => {
  const normalized = normalizeThreadRuntimeStatus(status);
  return normalized === 'running' || isThreadRuntimeWaiting(normalized);
};

export const didThreadRuntimeEnterBusyState = (
  previousStatus: unknown,
  nextStatus: unknown
): boolean => {
  const previous = normalizeThreadRuntimeStatus(previousStatus);
  const next = normalizeThreadRuntimeStatus(nextStatus);
  return previous !== next && !isThreadRuntimeBusy(previous) && isThreadRuntimeBusy(next);
};

export const isAssistantRuntimeRunning = (message: ChatMessage | null | undefined): boolean => {
  return isAssistantMessageRunning(message) || hasAssistantWaitingForCurrentOutput(message);
};

export const hasRunningAssistantMessage = (
  messages: ChatMessage[] | null | undefined
): boolean => {
  return hasActiveSubagentsAfterLatestUser(messages) || hasStreamingAssistantMessage(messages);
};

export const hasActiveSubagentsAfterLatestUser = (
  messages: ChatMessage[] | null | undefined
): boolean => {
  if (!Array.isArray(messages) || messages.length === 0) return false;
  const latestUserIndex = resolveLatestUserIndex(messages);
  const startIndex = latestUserIndex >= 0 ? latestUserIndex + 1 : 0;
  for (let index = messages.length - 1; index >= startIndex; index -= 1) {
    if (hasActiveSubagentItems(messages[index]?.subagents)) {
      return true;
    }
  }
  return false;
};

export const hasActiveBlockingSwarmAfterLatestUser = (
  messages: ChatMessage[] | null | undefined
): boolean => {
  if (!Array.isArray(messages) || messages.length === 0) return false;
  const latestUserIndex = resolveLatestUserIndex(messages);
  const startIndex = latestUserIndex >= 0 ? latestUserIndex + 1 : 0;
  for (let index = messages.length - 1; index >= startIndex; index -= 1) {
    const items = Array.isArray(messages[index]?.workflowItems)
      ? messages[index].workflowItems as unknown[]
      : [];
    if (items.some(isBlockingSwarmWorkflowItem)) return true;
  }
  return false;
};

export const hasStreamingAssistantMessage = (
  messages: ChatMessage[] | null | undefined
): boolean => {
  if (!Array.isArray(messages) || messages.length === 0) return false;
  const latestUserIndex = resolveLatestUserIndex(messages);
  const startIndex = latestUserIndex >= 0 ? latestUserIndex + 1 : 0;
  for (let index = messages.length - 1; index >= startIndex; index -= 1) {
    if (isAssistantRuntimeRunning(messages[index])) {
      return true;
    }
  }
  return false;
};

export const isSessionBusyFromSignals = (
  loading: unknown,
  messages: ChatMessage[] | null | undefined,
  threadStatus: unknown = null
): boolean => normalizeFlag(loading) || isThreadRuntimeBusy(threadStatus) || hasRunningAssistantMessage(messages);
