const CHAT_RENDER_EVENT_TYPES = new Set([
  'round_start',
  'received',
  'queued',
  'queue_enter',
  'queue_update',
  'queue_start',
  'queue_finish',
  'queue_fail',
  'progress',
  'channel_message',
  'message',
  'delta',
  'think_delta',
  'reasoning_delta',
  'llm_output_delta',
  'llm_request',
  'llm_stream_retry',
  'knowledge_request',
  'token_usage',
  'round_usage',
  'context_usage',
  'quota_usage',
  'plan_update',
  'question_panel',
  'final',
  'error',
  'turn_terminal',
  'tool_call',
  'tool_call_delta',
  'tool_output',
  'tool_output_delta',
  'tool_result',
  'approval_request',
  'approval_result',
  'approval_resolved',
  'slow_client',
  'compaction',
  'compaction_progress',
  'compaction_notice'
]);

const WATCH_ONLY_RENDER_EVENT_TYPES = new Set([
  'thread_control',
  'command_session_delta',
  'command_session_start',
  'command_session_status',
  'command_session_exit',
  'command_session_summary'
]);

const INTERACTIVE_COMMAND_SESSION_EVENT_TYPES = new Set([
  'command_session_delta',
  'command_session_start',
  'command_session_status',
  'command_session_exit',
  'command_session_summary'
]);

const INTERACTIVE_CANONICAL_SIDE_EFFECT_EVENT_TYPES = new Set([
  'thread_control',
  'workspace_update',
  'desktop_controller_hint',
  'desktop_controller_hint_done',
  'desktop_monitor_countdown',
  'desktop_monitor_countdown_done'
]);

const INTERACTIVE_CONTROL_EVENT_TYPES = new Set([
  'heartbeat',
  'ping',
  'thread_status',
  'thread_closed'
]);

export const normalizeProjectionOnlyEventType = (value: unknown): string =>
  String(value || '').trim().toLowerCase();

const isTeamOrSubagentRuntimeEvent = (eventType: string): boolean =>
  eventType.startsWith('team_') || eventType.startsWith('subagent_');

const isInteractiveControlEvent = (eventType: string): boolean =>
  INTERACTIVE_CONTROL_EVENT_TYPES.has(eventType);

export const shouldUseProjectionOnlyWatchStreamEvent = (
  eventType: unknown,
  options: { interactiveActive?: boolean } = {}
): boolean => {
  const normalized = normalizeProjectionOnlyEventType(eventType);
  if (!normalized) return false;
  if (isInteractiveControlEvent(normalized)) return false;
  if (Boolean(options.interactiveActive)) return true;
  if (CHAT_RENDER_EVENT_TYPES.has(normalized)) return true;
  if (WATCH_ONLY_RENDER_EVENT_TYPES.has(normalized)) return true;
  if (isTeamOrSubagentRuntimeEvent(normalized)) return true;
  return true;
};

export const shouldUseProjectionOnlyInteractiveStreamEvent = (
  eventType: unknown,
  options: { terminalLlmOutput?: boolean } = {}
): boolean => {
  void options;
  const normalized = normalizeProjectionOnlyEventType(eventType);
  if (!normalized) return false;
  if (isInteractiveControlEvent(normalized)) return false;
  if (normalized === 'llm_output') return true;
  if (CHAT_RENDER_EVENT_TYPES.has(normalized)) return true;
  if (INTERACTIVE_COMMAND_SESSION_EVENT_TYPES.has(normalized)) return true;
  if (INTERACTIVE_CANONICAL_SIDE_EFFECT_EVENT_TYPES.has(normalized)) return true;
  if (isTeamOrSubagentRuntimeEvent(normalized)) return true;
  return true;
};
