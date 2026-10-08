import type {
  ChatRuntimeMessageProjection,
  ChatRuntimeMessageStatus,
  ChatRuntimeProjection
} from './chatRuntimeTypes';
import {
  selectSessionBusy,
  selectVisibleMessageProjections
} from './chatRuntimeSelectors';

type ChatRuntimeShadowRole = 'user' | 'assistant';

export type ChatRuntimeShadowIssueCode =
  | 'legacy_duplicate_message'
  | 'projection_duplicate_message'
  | 'legacy_missing_projected_message'
  | 'projection_missing_legacy_message'
  | 'message_order_drift'
  | 'message_role_drift'
  | 'message_content_drift'
  | 'message_reasoning_drift'
  | 'message_status_drift'
  | 'busy_state_drift';

export type ChatRuntimeShadowIssue = {
  code: ChatRuntimeShadowIssueCode;
  message: string;
  projectedIndex?: number;
  legacyIndex?: number;
  role?: ChatRuntimeShadowRole;
  key?: string;
  details?: Record<string, unknown>;
};

export type ChatRuntimeShadowReport = {
  ok: boolean;
  sessionId: string;
  phase: string;
  checkedAt: number;
  projectedCount: number;
  legacyCount: number;
  matchedCount: number;
  fingerprint: string;
  issues: ChatRuntimeShadowIssue[];
};

export type CompareChatRuntimeShadowOptions = {
  projection: ChatRuntimeProjection | null | undefined;
  sessionId: unknown;
  legacyMessages: unknown[] | null | undefined;
  legacyBusy?: boolean | null;
  phase?: string;
  issueLimit?: number;
};

type ShadowMessage = {
  index: number;
  role: ChatRuntimeShadowRole;
  content: string;
  reasoning: string;
  status: string;
  primaryKey: string;
  keys: string[];
  raw?: unknown;
};

type ShadowMatch = {
  projected: ShadowMessage;
  legacy: ShadowMessage;
};

const DEFAULT_ISSUE_LIMIT = 40;
const ACTIVE_STATUS = 'active';
const ACTIVE_LEGACY_WORKFLOW_STATUSES = new Set(['loading', 'pending', 'running', 'streaming']);

export const compareChatRuntimeShadow = (
  options: CompareChatRuntimeShadowOptions
): ChatRuntimeShadowReport => {
  const sessionId = normalizeId(options.sessionId);
  const projected = buildProjectedShadowMessages(
    selectVisibleMessageProjections(options.projection, sessionId)
  );
  const legacy = buildLegacyShadowMessages(options.legacyMessages);
  const issueLimit = Math.max(1, Number(options.issueLimit) || DEFAULT_ISSUE_LIMIT);
  const issues: ChatRuntimeShadowIssue[] = [];
  const pushIssue = (issue: ChatRuntimeShadowIssue): void => {
    if (issues.length < issueLimit) {
      issues.push(issue);
    }
  };

  collectDuplicateIssues(projected, 'projection', pushIssue);
  collectDuplicateIssues(legacy, 'legacy', pushIssue);

  const matches = matchShadowMessages(projected, legacy);
  const matchedProjected = new Set(matches.map((match) => match.projected.index));
  const matchedLegacy = new Set(matches.map((match) => match.legacy.index));

  projected.forEach((message) => {
    if (matchedProjected.has(message.index)) return;
    pushIssue({
      code: 'legacy_missing_projected_message',
      message: 'legacy render messages do not contain a projected runtime message',
      projectedIndex: message.index,
      role: message.role,
      key: message.primaryKey,
      details: summarizeShadowMessage(message)
    });
  });

  legacy.forEach((message) => {
    if (matchedLegacy.has(message.index)) return;
    pushIssue({
      code: 'projection_missing_legacy_message',
      message: 'runtime projection does not contain a legacy render message',
      legacyIndex: message.index,
      role: message.role,
      key: message.primaryKey,
      details: summarizeShadowMessage(message)
    });
  });

  collectOrderIssue(matches, pushIssue);
  collectMatchedDriftIssues(matches, pushIssue);
  collectBusyIssue(options, pushIssue);

  const fingerprint = buildShadowFingerprint(issues);
  return {
    ok: issues.length === 0,
    sessionId,
    phase: normalizeId(options.phase) || 'shadow',
    checkedAt: Date.now(),
    projectedCount: projected.length,
    legacyCount: legacy.length,
    matchedCount: matches.length,
    fingerprint,
    issues
  };
};

export const summarizeChatRuntimeShadowReport = (
  report: ChatRuntimeShadowReport
): Record<string, unknown> => ({
  ok: report.ok,
  sessionId: report.sessionId,
  phase: report.phase,
  projectedCount: report.projectedCount,
  legacyCount: report.legacyCount,
  matchedCount: report.matchedCount,
  fingerprint: report.fingerprint,
  issueCount: report.issues.length,
  issues: report.issues.slice(0, 8)
});

const buildProjectedShadowMessages = (
  messages: ChatRuntimeMessageProjection[]
): ShadowMessage[] =>
  messages
    .filter((message) => message.role === 'user' || message.role === 'assistant')
    .map((message, index) => {
      const keys = uniqueKeys([
        message.id ? `message:${message.id}` : '',
        message.legacyKey ? `message:${message.legacyKey}` : '',
        eventKeyFromId(message.id),
        eventKeyFromId(message.legacyKey),
        message.role === 'user' && message.userTurnId
          ? `user-turn:${message.userTurnId}`
          : '',
        message.role === 'assistant' && message.modelTurnId
          ? `assistant-turn:${message.modelTurnId}`
          : ''
      ]);
      return {
        index,
        role: message.role as ChatRuntimeShadowRole,
        content: normalizeContent(message.content),
        reasoning: normalizeContent(message.reasoning),
        status: normalizeProjectionStatus(message.status),
        primaryKey: keys[0] || `projection-index:${index}`,
        keys,
        raw: message.raw
      };
    });

const buildLegacyShadowMessages = (
  messages: unknown[] | null | undefined
): ShadowMessage[] => {
  const source = Array.isArray(messages) ? messages : [];
  const result: ShadowMessage[] = [];
  source.forEach((raw, sourceIndex) => {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return;
    const record = raw as Record<string, unknown>;
    if (record.hiddenInternal === true || record.hidden_internal === true) return;
    const role = normalizeRole(record.role);
    if (!role) return;
    const index = result.length;
    const messageId = firstId(record.message_id, record.messageId, record.id);
    const clientMessageId = firstId(record.client_message_id, record.clientMessageId);
    const streamEventId = normalizePositiveInteger(record.stream_event_id ?? record.streamEventId);
    const streamRound = normalizePositiveInteger(record.stream_round ?? record.streamRound);
    const modelTurnId = firstId(record.model_turn_id, record.modelTurnId);
    const keys = uniqueKeys([
      messageId ? `message:${messageId}` : '',
      clientMessageId ? `message:${clientMessageId}` : '',
      clientMessageId ? `client:${clientMessageId}` : '',
      streamEventId !== null ? `event:${streamEventId}` : '',
      role === 'assistant' && modelTurnId ? `assistant-turn:${modelTurnId}` : '',
      role === 'assistant' && streamRound !== null ? `assistant-turn:legacy-model-turn:round:${streamRound}` : '',
      role === 'user' && streamRound !== null ? `user-turn:legacy-user-turn:round:${streamRound}` : ''
    ]);
    result.push({
      index,
      role,
      content: normalizeContent(record.content),
      reasoning: normalizeContent(record.reasoning),
      status: normalizeLegacyStatus(record),
      primaryKey: keys[0] || `legacy-index:${sourceIndex}`,
      keys,
      raw
    });
  });
  return result;
};

const collectDuplicateIssues = (
  messages: ShadowMessage[],
  source: 'legacy' | 'projection',
  pushIssue: (issue: ChatRuntimeShadowIssue) => void
): void => {
  const byPrimaryKey = new Map<string, ShadowMessage[]>();
  messages.forEach((message) => {
    if (!isStableShadowKey(message.primaryKey)) return;
    const current = byPrimaryKey.get(message.primaryKey) || [];
    current.push(message);
    byPrimaryKey.set(message.primaryKey, current);
  });
  byPrimaryKey.forEach((items, key) => {
    if (items.length <= 1) return;
    pushIssue({
      code: source === 'legacy' ? 'legacy_duplicate_message' : 'projection_duplicate_message',
      message: `${source} messages contain duplicate stable message identities`,
      role: items[0]?.role,
      key,
      details: {
        indexes: items.map((item) => item.index),
        count: items.length
      }
    });
  });
};

const matchShadowMessages = (
  projected: ShadowMessage[],
  legacy: ShadowMessage[]
): ShadowMatch[] => {
  const matches: ShadowMatch[] = [];
  const usedLegacy = new Set<number>();
  const legacyByKey = new Map<string, ShadowMessage[]>();
  legacy.forEach((message) => {
    message.keys.forEach((key) => {
      if (!isStableShadowKey(key)) return;
      const bucket = legacyByKey.get(key) || [];
      bucket.push(message);
      legacyByKey.set(key, bucket);
    });
  });

  projected.forEach((projectedMessage) => {
    const rawMatch = legacy.find(
      (legacyMessage) =>
        !usedLegacy.has(legacyMessage.index) &&
        projectedMessage.raw !== undefined &&
        projectedMessage.raw === legacyMessage.raw
    );
    if (rawMatch) {
      usedLegacy.add(rawMatch.index);
      matches.push({ projected: projectedMessage, legacy: rawMatch });
      return;
    }

    for (const key of projectedMessage.keys) {
      const candidate = (legacyByKey.get(key) || []).find(
        (legacyMessage) => !usedLegacy.has(legacyMessage.index)
      );
      if (!candidate) continue;
      usedLegacy.add(candidate.index);
      matches.push({ projected: projectedMessage, legacy: candidate });
      return;
    }
  });

  return matches;
};

const collectOrderIssue = (
  matches: ShadowMatch[],
  pushIssue: (issue: ChatRuntimeShadowIssue) => void
): void => {
  let previousLegacyIndex = -1;
  for (const match of matches) {
    if (match.legacy.index < previousLegacyIndex) {
      pushIssue({
        code: 'message_order_drift',
        message: 'projected and legacy message orders disagree for matched messages',
        details: {
          projectedOrder: matches.map((item) => item.projected.index),
          legacyOrder: matches.map((item) => item.legacy.index)
        }
      });
      return;
    }
    previousLegacyIndex = match.legacy.index;
  }
};

const collectMatchedDriftIssues = (
  matches: ShadowMatch[],
  pushIssue: (issue: ChatRuntimeShadowIssue) => void
): void => {
  matches.forEach(({ projected, legacy }) => {
    if (projected.role !== legacy.role) {
      pushIssue({
        code: 'message_role_drift',
        message: 'matched messages have different roles',
        projectedIndex: projected.index,
        legacyIndex: legacy.index,
        key: projected.primaryKey,
        details: {
          projectedRole: projected.role,
          legacyRole: legacy.role
        }
      });
    }
    if (projected.content !== legacy.content) {
      pushIssue({
        code: 'message_content_drift',
        message: 'matched messages have different content',
        projectedIndex: projected.index,
        legacyIndex: legacy.index,
        role: projected.role,
        key: projected.primaryKey,
        details: {
          projectedLength: projected.content.length,
          legacyLength: legacy.content.length
        }
      });
    }
    if (projected.reasoning !== legacy.reasoning) {
      pushIssue({
        code: 'message_reasoning_drift',
        message: 'matched assistant messages have different reasoning content',
        projectedIndex: projected.index,
        legacyIndex: legacy.index,
        role: projected.role,
        key: projected.primaryKey,
        details: {
          projectedLength: projected.reasoning.length,
          legacyLength: legacy.reasoning.length
        }
      });
    }
    if (projected.status !== legacy.status) {
      pushIssue({
        code: 'message_status_drift',
        message: 'matched messages have different runtime status classes',
        projectedIndex: projected.index,
        legacyIndex: legacy.index,
        role: projected.role,
        key: projected.primaryKey,
        details: {
          projectedStatus: projected.status,
          legacyStatus: legacy.status
        }
      });
    }
  });
};

const collectBusyIssue = (
  options: CompareChatRuntimeShadowOptions,
  pushIssue: (issue: ChatRuntimeShadowIssue) => void
): void => {
  if (options.legacyBusy === undefined || options.legacyBusy === null) return;
  const sessionId = normalizeId(options.sessionId);
  const projectedBusy = selectSessionBusy(options.projection, sessionId);
  const legacyBusy = Boolean(options.legacyBusy);
  if (projectedBusy === legacyBusy) return;
  pushIssue({
    code: 'busy_state_drift',
    message: 'projected busy state and legacy busy state disagree',
    details: {
      projectedBusy,
      legacyBusy
    }
  });
};

const summarizeShadowMessage = (message: ShadowMessage): Record<string, unknown> => ({
  index: message.index,
  role: message.role,
  status: message.status,
  key: message.primaryKey,
  contentLength: message.content.length,
  reasoningLength: message.reasoning.length
});

const normalizeRole = (value: unknown): ChatRuntimeShadowRole | null => {
  const role = String(value ?? '').trim().toLowerCase();
  if (role === 'user' || role === 'assistant') return role;
  return null;
};

const normalizeProjectionStatus = (status: ChatRuntimeMessageStatus): string => {
  if (
    status === 'placeholder' ||
    status === 'queued' ||
    status === 'waiting_first_output' ||
    status === 'streaming' ||
    status === 'tooling'
  ) {
    return ACTIVE_STATUS;
  }
  return status;
};

const normalizeLegacyStatus = (message: Record<string, unknown>): string => {
  const status = String(message.status ?? '').trim().toLowerCase();
  if (status === 'failed') return 'failed';
  if (status === 'cancelled' || status === 'canceled') return 'cancelled';
  if (
    message.stream_incomplete === true ||
    message.reasoningStreaming === true ||
    message.reasoning_streaming === true ||
    message.workflowStreaming === true ||
    message.workflow_streaming === true ||
    hasActiveWorkflowItems(message.workflowItems)
  ) {
    return ACTIVE_STATUS;
  }
  return 'final';
};

const hasActiveWorkflowItems = (value: unknown): boolean => {
  if (!Array.isArray(value)) return false;
  return value.some((item) => {
    if (!item || typeof item !== 'object' || Array.isArray(item)) return false;
    const status = String((item as Record<string, unknown>).status ?? '').trim().toLowerCase();
    return ACTIVE_LEGACY_WORKFLOW_STATUSES.has(status);
  });
};

const normalizeContent = (value: unknown): string =>
  String(value ?? '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');

const firstId = (...values: unknown[]): string => {
  for (const value of values) {
    const id = normalizeId(value);
    if (id) return id;
  }
  return '';
};

const normalizeId = (value: unknown): string => String(value ?? '').trim();

const normalizePositiveInteger = (value: unknown): number | null => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : null;
};

const uniqueKeys = (keys: string[]): string[] => {
  const seen = new Set<string>();
  const result: string[] = [];
  keys.forEach((key) => {
    const normalized = normalizeId(key);
    if (!normalized || seen.has(normalized)) return;
    seen.add(normalized);
    result.push(normalized);
  });
  return result;
};

const eventKeyFromId = (value: unknown): string => {
  const id = normalizeId(value);
  const match = /^legacy-event:(\d+)$/.exec(id);
  return match ? `event:${match[1]}` : '';
};

const isStableShadowKey = (key: string): boolean =>
  key.startsWith('message:') ||
  key.startsWith('client:') ||
  key.startsWith('event:') ||
  key.startsWith('user-turn:') ||
  key.startsWith('assistant-turn:');

const buildShadowFingerprint = (issues: ChatRuntimeShadowIssue[]): string => {
  if (issues.length === 0) return 'ok';
  const raw = issues
    .map((issue) => [
      issue.code,
      issue.projectedIndex ?? '',
      issue.legacyIndex ?? '',
      issue.role ?? '',
      issue.key ?? ''
    ].join(':'))
    .join('|');
  return hashText(raw);
};

const hashText = (value: string): string => {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
};
