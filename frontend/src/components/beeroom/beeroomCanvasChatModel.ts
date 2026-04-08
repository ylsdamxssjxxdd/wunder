export type MissionChatMessage = {
  key: string;
  senderName: string;
  senderAgentId: string;
  mention: string;
  body: string;
  meta: string;
  time: number;
  timeLabel: string;
  tone: 'mother' | 'worker' | 'system' | 'user';
  sortOrder?: number;
};

export const BEEROOM_SUBAGENT_REQUEST_SORT_ORDER = -20;
export const BEEROOM_SUBAGENT_REPLY_SORT_ORDER = -10;

const normalizeSortOrder = (value: unknown): number => {
  const normalized = Number(value);
  return Number.isFinite(normalized) ? normalized : 0;
};

export const compareMissionChatMessages = (left: MissionChatMessage, right: MissionChatMessage) =>
  left.time - right.time ||
  normalizeSortOrder(left.sortOrder) - normalizeSortOrder(right.sortOrder) ||
  left.key.localeCompare(right.key);

export type ComposerTargetOption = {
  agentId: string;
  label: string;
  role: 'mother' | 'worker';
};

export type DispatchRuntimeStatus =
  | 'idle'
  | 'queued'
  | 'running'
  | 'awaiting_approval'
  | 'resuming'
  | 'stopped'
  | 'completed'
  | 'failed';

export type DispatchApprovalItem = {
  approval_id: string;
  session_id: string;
  tool: string;
  summary: string;
};
