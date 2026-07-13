export type MissionChatMessage = {
  key: string;
  remoteKey?: string;
  sessionId?: string;
  clientMessageId?: string;
  userTurnId?: string;
  modelTurnId?: string;
  turnOrder?: number;
  messageOrder?: number;
  senderName: string;
  senderAgentId: string;
  avatarImageUrl?: string;
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

const resolveToneSortOrder = (message: MissionChatMessage): number => {
  if (message.tone === 'user') return -1;
  if (message.tone === 'system') return 2;
  return 0;
};

const normalizePositiveOrder = (value: unknown): number | null => {
  const normalized = Number(value);
  return Number.isFinite(normalized) && normalized > 0 ? normalized : null;
};

const resolveMessageTurnOrder = (message: MissionChatMessage): number | null => {
  const explicit = normalizePositiveOrder(message.turnOrder);
  if (explicit !== null) return explicit;
  const userTurnId = String(message.userTurnId || '').trim();
  const roundMatch = userTurnId.match(/(?:^|:)round:(\d+)(?::|$)/i);
  return roundMatch ? normalizePositiveOrder(roundMatch[1]) : null;
};

export const compareMissionChatMessages = (left: MissionChatMessage, right: MissionChatMessage) => {
  const leftTurnOrder = resolveMessageTurnOrder(left);
  const rightTurnOrder = resolveMessageTurnOrder(right);
  if (leftTurnOrder !== null && rightTurnOrder !== null && leftTurnOrder !== rightTurnOrder) {
    return leftTurnOrder - rightTurnOrder;
  }
  if (leftTurnOrder !== null && rightTurnOrder === leftTurnOrder) {
    return (
      normalizeSortOrder(left.messageOrder) - normalizeSortOrder(right.messageOrder) ||
      resolveToneSortOrder(left) - resolveToneSortOrder(right) ||
      left.time - right.time ||
      normalizeSortOrder(left.sortOrder) - normalizeSortOrder(right.sortOrder) ||
      left.key.localeCompare(right.key)
    );
  }
  // Optimistic and canonical identities can land on the two sides at
  // different times. Keep the mixed pair chronological until both settle.
  if ((leftTurnOrder === null) !== (rightTurnOrder === null)) {
    return (
      left.time - right.time ||
      resolveToneSortOrder(left) - resolveToneSortOrder(right) ||
      normalizeSortOrder(left.messageOrder) - normalizeSortOrder(right.messageOrder) ||
      left.key.localeCompare(right.key)
    );
  }
  return (
    left.time - right.time ||
    resolveToneSortOrder(left) - resolveToneSortOrder(right) ||
    normalizeSortOrder(left.messageOrder) - normalizeSortOrder(right.messageOrder) ||
    normalizeSortOrder(left.sortOrder) - normalizeSortOrder(right.sortOrder) ||
    left.key.localeCompare(right.key)
  );
};

export const collapseMissionChatAssistantTurns = (messages: MissionChatMessage[]): MissionChatMessage[] => {
  const ordered = [...messages].sort(compareMissionChatMessages);
  const collapsed: MissionChatMessage[] = [];
  let trailingAssistant: MissionChatMessage | null = null;

  const flushTrailingAssistant = () => {
    if (!trailingAssistant) return;
    collapsed.push(trailingAssistant);
    trailingAssistant = null;
  };

  ordered.forEach((message) => {
    if (message.tone === 'user') {
      flushTrailingAssistant();
      collapsed.push(message);
      return;
    }
    trailingAssistant = message;
  });

  flushTrailingAssistant();
  return collapsed;
};

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
