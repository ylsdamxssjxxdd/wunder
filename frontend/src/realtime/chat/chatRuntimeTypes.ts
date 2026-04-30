export type ChatRuntimeConnectionState = 'connected' | 'reconnecting' | 'offline';

export type ChatSessionRuntimeStatus =
  | 'not_loaded'
  | 'idle'
  | 'queued'
  | 'running'
  | 'waiting_approval'
  | 'waiting_user_input'
  | 'finalizing'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'reconnecting'
  | 'offline'
  | 'system_error';

export type ChatRuntimeBusyReason =
  | 'queued'
  | 'waiting_first_output'
  | 'streaming'
  | 'tool_running'
  | 'waiting_approval'
  | 'waiting_user_input'
  | 'finalizing'
  | 'cancelling'
  | 'reconnecting'
  | 'syncing';

export type ChatRuntimeMessageRole = 'user' | 'assistant' | 'system';

export type ChatRuntimeMessageStatus =
  | 'placeholder'
  | 'waiting_first_output'
  | 'streaming'
  | 'tooling'
  | 'final'
  | 'failed'
  | 'cancelled';

export type ChatRuntimeEventType =
  | 'connection_state'
  | 'client_message_submitted'
  | 'legacy_messages_reconciled'
  | 'session_snapshot'
  | 'session_runtime'
  | 'user_message_created'
  | 'assistant_message_created'
  | 'assistant_delta'
  | 'assistant_reasoning_delta'
  | 'assistant_final'
  | 'tool_call_started'
  | 'tool_call_delta'
  | 'tool_call_completed'
  | 'tool_call_failed'
  | 'turn_completed'
  | 'turn_failed'
  | 'turn_cancelled'
  | 'session_idle'
  | 'sync_required';

export type ChatRuntimeEventSource = 'ws' | 'local' | 'snapshot' | 'legacy' | 'test';

export type ChatRuntimeRawMessage = Record<string, unknown>;

export type ChatRuntimeEvent = {
  event_type: ChatRuntimeEventType | string;
  source?: ChatRuntimeEventSource | string;
  strict?: boolean;
  session_id?: unknown;
  agent_id?: unknown;
  event_id?: unknown;
  event_seq?: unknown;
  snapshot_seq?: unknown;
  user_turn_id?: unknown;
  model_turn_id?: unknown;
  message_id?: unknown;
  role?: unknown;
  content?: unknown;
  reasoning?: unknown;
  delta?: unknown;
  reasoning_delta?: unknown;
  runtime_status?: unknown;
  is_terminal?: unknown;
  created_at?: unknown;
  payload?: Record<string, unknown> | null;
  messages?: ChatRuntimeRawMessage[];
  running?: unknown;
  loading?: unknown;
};

export type ChatRuntimeViolation = {
  code: string;
  message: string;
  eventSeq: number | null;
  eventType: string;
  messageId?: string;
  userTurnId?: string;
  modelTurnId?: string;
};

export type ChatRuntimeQuarantinedEvent = {
  reason: string;
  eventType: string;
  eventSeq: number | null;
  eventId: string;
  receivedAt: number;
  event: ChatRuntimeEvent;
};

export type ChatRuntimeMessageProjection = {
  id: string;
  role: ChatRuntimeMessageRole;
  content: string;
  reasoning: string;
  status: ChatRuntimeMessageStatus;
  createdAt: string;
  createdSeq: number;
  updatedSeq: number;
  userTurnId: string;
  modelTurnId: string;
  final: boolean;
  failed: boolean;
  cancelled: boolean;
  legacyKey?: string;
  raw?: ChatRuntimeRawMessage;
};

export type ChatRuntimeUserTurnProjection = {
  id: string;
  createdSeq: number;
  messageIds: string[];
  modelTurnIds: string[];
  status: 'created' | 'accepted' | 'dispatched' | 'model_running' | 'waiting_user_input' | 'completed' | 'failed' | 'cancelled';
};

export type ChatRuntimeModelTurnProjection = {
  id: string;
  userTurnId: string;
  createdSeq: number;
  messageIds: string[];
  finalMessageId: string;
  status: 'created' | 'waiting_first_output' | 'streaming' | 'tool_running' | 'finalizing' | 'completed' | 'failed' | 'cancelled';
};

export type ChatRuntimeSessionProjection = {
  sessionId: string;
  agentId: string;
  appliedSeq: number;
  snapshotSeq: number;
  localSeq: number;
  syncRequired: boolean;
  connectionState: ChatRuntimeConnectionState;
  runtimeStatus: ChatSessionRuntimeStatus;
  busyReason: ChatRuntimeBusyReason | null;
  eventIdIndex: Record<string, true>;
  userTurns: string[];
  modelTurns: string[];
  messages: string[];
  messageById: Record<string, ChatRuntimeMessageProjection>;
  userTurnById: Record<string, ChatRuntimeUserTurnProjection>;
  modelTurnById: Record<string, ChatRuntimeModelTurnProjection>;
  invariantViolations: ChatRuntimeViolation[];
  quarantinedEvents: ChatRuntimeQuarantinedEvent[];
};

export type ChatRuntimeDebugEvent = {
  receivedAt: number;
  sessionId: string;
  eventType: string;
  eventSeq: number | null;
  eventId: string;
  beforeSummary: string;
  afterSummary: string;
  violationCount: number;
};

export type ChatRuntimeProjection = {
  activeSessionId: string | null;
  sessions: Record<string, ChatRuntimeSessionProjection>;
  debugEvents: ChatRuntimeDebugEvent[];
};

export type ChatRuntimeApplyResult = {
  applied: boolean;
  ignored: boolean;
  quarantined: boolean;
  sessionId: string;
  eventSeq: number | null;
  reason?: string;
};
