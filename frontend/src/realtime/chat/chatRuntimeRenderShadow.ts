import type { ChatRuntimeRenderableMessage } from './chatRuntimeRenderAdapter';

type RenderShadowRole = 'user' | 'assistant';

export type ChatRuntimeRenderShadowIssueCode =
  | 'render_key_drift'
  | 'render_missing_projection_message'
  | 'render_missing_legacy_message'
  | 'render_order_drift'
  | 'render_content_drift'
  | 'render_reasoning_drift'
  | 'render_streaming_flag_drift'
  | 'render_workflow_drift'
  | 'render_subagent_drift';

export type ChatRuntimeRenderShadowIssue = {
  code: ChatRuntimeRenderShadowIssueCode;
  message: string;
  legacyIndex?: number;
  projectionIndex?: number;
  key?: string;
  role?: RenderShadowRole;
  details?: Record<string, unknown>;
};

export type ChatRuntimeRenderShadowReport = {
  ok: boolean;
  sessionId: string;
  checkedAt: number;
  legacyCount: number;
  projectionCount: number;
  matchedCount: number;
  fingerprint: string;
  issues: ChatRuntimeRenderShadowIssue[];
};

export type CompareChatRuntimeRenderShadowOptions = {
  sessionId: unknown;
  legacy: ChatRuntimeRenderableMessage[] | null | undefined;
  projection: ChatRuntimeRenderableMessage[] | null | undefined;
  issueLimit?: number;
};

type RenderShadowMessage = {
  index: number;
  role: RenderShadowRole;
  key: string;
  identityKeys: string[];
  content: string;
  reasoning: string;
  streaming: boolean;
  workflow: RenderShadowCollectionSummary;
  subagents: RenderShadowCollectionSummary;
};

type RenderShadowMatch = {
  legacy: RenderShadowMessage;
  projection: RenderShadowMessage;
};

type RenderShadowCollectionSummary = {
  count: number;
  activeCount: number;
  signature: string;
  entries: string[];
};

const DEFAULT_ISSUE_LIMIT = 40;
const EMPTY_COLLECTION_SIGNATURE = 'empty';
const ACTIVE_COLLECTION_STATUSES = new Set([
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

export const compareChatRuntimeRenderShadow = (
  options: CompareChatRuntimeRenderShadowOptions
): ChatRuntimeRenderShadowReport => {
  const legacy = buildRenderShadowMessages(options.legacy);
  const projection = buildRenderShadowMessages(options.projection);
  const issueLimit = Math.max(1, Number(options.issueLimit) || DEFAULT_ISSUE_LIMIT);
  const issues: ChatRuntimeRenderShadowIssue[] = [];
  const pushIssue = (issue: ChatRuntimeRenderShadowIssue): void => {
    if (issues.length < issueLimit) {
      issues.push(issue);
    }
  };

  const matches = matchRenderShadowMessages(legacy, projection);
  const matchedLegacy = new Set(matches.map((match) => match.legacy.index));
  const matchedProjection = new Set(matches.map((match) => match.projection.index));

  legacy.forEach((message) => {
    if (matchedLegacy.has(message.index)) return;
    pushIssue({
      code: 'render_missing_projection_message',
      message: 'projection renderable messages do not contain a legacy renderable message',
      legacyIndex: message.index,
      role: message.role,
      key: message.key,
      details: summarizeRenderShadowMessage(message)
    });
  });

  projection.forEach((message) => {
    if (matchedProjection.has(message.index)) return;
    pushIssue({
      code: 'render_missing_legacy_message',
      message: 'legacy renderable messages do not contain a projection renderable message',
      projectionIndex: message.index,
      role: message.role,
      key: message.key,
      details: summarizeRenderShadowMessage(message)
    });
  });

  collectMatchedRenderDrift(matches, pushIssue);
  collectRenderOrderDrift(matches, pushIssue);

  const fingerprint = buildRenderShadowFingerprint(issues);
  return {
    ok: issues.length === 0,
    sessionId: normalizeText(options.sessionId),
    checkedAt: Date.now(),
    legacyCount: legacy.length,
    projectionCount: projection.length,
    matchedCount: matches.length,
    fingerprint,
    issues
  };
};

export const summarizeChatRuntimeRenderShadowReport = (
  report: ChatRuntimeRenderShadowReport
): Record<string, unknown> => ({
  ok: report.ok,
  sessionId: report.sessionId,
  legacyCount: report.legacyCount,
  projectionCount: report.projectionCount,
  matchedCount: report.matchedCount,
  fingerprint: report.fingerprint,
  issueCount: report.issues.length,
  issues: report.issues.slice(0, 8)
});

const buildRenderShadowMessages = (
  renderable: ChatRuntimeRenderableMessage[] | null | undefined
): RenderShadowMessage[] =>
  (Array.isArray(renderable) ? renderable : [])
    .map((item, index) => {
      const message = item?.message || {};
      const role = normalizeRole(message.role);
      if (!role) return null;
      const identityKeys = uniqueKeys([
        item.key ? `render:${item.key}` : '',
        firstText(message.__runtime_message_id, message.message_id, message.messageId, message.id)
          ? `message:${firstText(message.__runtime_message_id, message.message_id, message.messageId, message.id)}`
          : '',
        role === 'assistant' && firstText(message.__runtime_model_turn_id, message.model_turn_id, message.modelTurnId)
          ? `assistant-turn:${firstText(message.__runtime_model_turn_id, message.model_turn_id, message.modelTurnId)}`
          : '',
        role === 'user' && firstText(message.__runtime_user_turn_id, message.user_turn_id, message.userTurnId)
          ? `user-turn:${firstText(message.__runtime_user_turn_id, message.user_turn_id, message.userTurnId)}`
          : ''
      ]);
      return {
        index,
        role,
        key: String(item.key || '').trim(),
        identityKeys,
        content: normalizeContent(message.content),
        reasoning: normalizeContent(message.reasoning),
        streaming: normalizeFlag(message.stream_incomplete) ||
          normalizeFlag(message.workflowStreaming) ||
          normalizeFlag(message.reasoningStreaming),
        workflow: summarizeWorkflowCollection(message.workflowItems),
        subagents: summarizeSubagentCollection(message.subagents)
      };
    })
    .filter((message): message is RenderShadowMessage => Boolean(message));

const matchRenderShadowMessages = (
  legacy: RenderShadowMessage[],
  projection: RenderShadowMessage[]
): RenderShadowMatch[] => {
  const projectionByKey = new Map<string, RenderShadowMessage[]>();
  projection.forEach((message) => {
    message.identityKeys.forEach((key) => {
      const current = projectionByKey.get(key) || [];
      current.push(message);
      projectionByKey.set(key, current);
    });
  });

  const usedProjection = new Set<number>();
  const matches: RenderShadowMatch[] = [];
  legacy.forEach((legacyMessage) => {
    for (const key of legacyMessage.identityKeys) {
      const candidate = (projectionByKey.get(key) || []).find(
        (item) => !usedProjection.has(item.index) && item.role === legacyMessage.role
      );
      if (!candidate) continue;
      usedProjection.add(candidate.index);
      matches.push({ legacy: legacyMessage, projection: candidate });
      return;
    }
  });
  return matches;
};

const collectMatchedRenderDrift = (
  matches: RenderShadowMatch[],
  pushIssue: (issue: ChatRuntimeRenderShadowIssue) => void
): void => {
  matches.forEach((match) => {
    const key = firstText(match.legacy.identityKeys[0], match.projection.identityKeys[0]);
    if (match.legacy.key && match.projection.key && match.legacy.key !== match.projection.key) {
      pushIssue({
        code: 'render_key_drift',
        message: 'matched renderable messages use different vnode keys',
        legacyIndex: match.legacy.index,
        projectionIndex: match.projection.index,
        role: match.legacy.role,
        key,
        details: {
          legacyKey: match.legacy.key,
          projectionKey: match.projection.key
        }
      });
    }
    if (match.legacy.content !== match.projection.content) {
      pushIssue({
        code: 'render_content_drift',
        message: 'matched renderable messages expose different content',
        legacyIndex: match.legacy.index,
        projectionIndex: match.projection.index,
        role: match.legacy.role,
        key,
        details: {
          legacyLength: match.legacy.content.length,
          projectionLength: match.projection.content.length
        }
      });
    }
    if (match.legacy.reasoning !== match.projection.reasoning) {
      pushIssue({
        code: 'render_reasoning_drift',
        message: 'matched renderable messages expose different reasoning',
        legacyIndex: match.legacy.index,
        projectionIndex: match.projection.index,
        role: match.legacy.role,
        key,
        details: {
          legacyLength: match.legacy.reasoning.length,
          projectionLength: match.projection.reasoning.length
        }
      });
    }
    if (match.legacy.streaming !== match.projection.streaming) {
      pushIssue({
        code: 'render_streaming_flag_drift',
        message: 'matched renderable messages expose different streaming flags',
        legacyIndex: match.legacy.index,
        projectionIndex: match.projection.index,
        role: match.legacy.role,
        key,
        details: {
          legacyStreaming: match.legacy.streaming,
          projectionStreaming: match.projection.streaming
        }
      });
    }
    if (match.legacy.workflow.signature !== match.projection.workflow.signature) {
      pushIssue({
        code: 'render_workflow_drift',
        message: 'matched renderable messages expose different workflow timeline summaries',
        legacyIndex: match.legacy.index,
        projectionIndex: match.projection.index,
        role: match.legacy.role,
        key,
        details: {
          legacyWorkflow: match.legacy.workflow,
          projectionWorkflow: match.projection.workflow
        }
      });
    }
    if (match.legacy.subagents.signature !== match.projection.subagents.signature) {
      pushIssue({
        code: 'render_subagent_drift',
        message: 'matched renderable messages expose different subagent summaries',
        legacyIndex: match.legacy.index,
        projectionIndex: match.projection.index,
        role: match.legacy.role,
        key,
        details: {
          legacySubagents: match.legacy.subagents,
          projectionSubagents: match.projection.subagents
        }
      });
    }
  });
};

const collectRenderOrderDrift = (
  matches: RenderShadowMatch[],
  pushIssue: (issue: ChatRuntimeRenderShadowIssue) => void
): void => {
  const ordered = [...matches].sort((left, right) => left.legacy.index - right.legacy.index);
  for (let index = 1; index < ordered.length; index += 1) {
    if (ordered[index - 1].projection.index <= ordered[index].projection.index) continue;
    pushIssue({
      code: 'render_order_drift',
      message: 'matched renderable messages appear in a different order',
      legacyIndex: ordered[index].legacy.index,
      projectionIndex: ordered[index].projection.index,
      role: ordered[index].legacy.role,
      key: ordered[index].legacy.identityKeys[0],
      details: {
        previousLegacyIndex: ordered[index - 1].legacy.index,
        previousProjectionIndex: ordered[index - 1].projection.index
      }
    });
    return;
  }
};

const summarizeRenderShadowMessage = (
  message: RenderShadowMessage
): Record<string, unknown> => ({
  role: message.role,
  key: message.key,
  identityKeys: message.identityKeys.slice(0, 4),
  contentLength: message.content.length,
  reasoningLength: message.reasoning.length,
  streaming: message.streaming,
  workflowCount: message.workflow.count,
  workflowActiveCount: message.workflow.activeCount,
  subagentCount: message.subagents.count,
  subagentActiveCount: message.subagents.activeCount
});

const summarizeWorkflowCollection = (value: unknown): RenderShadowCollectionSummary => {
  const entries = normalizeRecordArray(value)
    .map((item) => {
      const type = normalizeStatus(
        item.eventType ??
          item.event_type ??
          item.sourceEventType ??
          item.source_event_type ??
          item.kind
      );
      const status = normalizeStatus(item.status);
      const ref = firstText(
        item.toolCallId,
        item.tool_call_id,
        item.commandSessionId,
        item.command_session_id,
        item.approvalId,
        item.approval_id,
        item.sessionId,
        item.session_id,
        item.runId,
        item.run_id,
        item.dispatchId,
        item.dispatch_id,
        item.taskId,
        item.task_id,
        item.toolName,
        item.tool,
        type
      );
      const tool = normalizeStatus(item.toolName ?? item.tool ?? item.name);
      return [
        type || 'workflow',
        status || 'unknown',
        ref || 'no-ref',
        tool || 'no-tool'
      ].join('/');
    })
    .sort();
  return buildCollectionSummary(entries);
};

const summarizeSubagentCollection = (value: unknown): RenderShadowCollectionSummary => {
  const entries = normalizeRecordArray(value)
    .map((item) => {
      const agentState = normalizeRecord(item.agent_state ?? item.agentState);
      const status = normalizeStatus(item.status ?? agentState.status);
      const ref = firstText(
        item.key,
        item.run_id,
        item.runId,
        item.session_id,
        item.sessionId,
        item.dispatch_id,
        item.dispatchId
      );
      const terminal = normalizeFlag(item.terminal) ? 'terminal' : 'open';
      const failed = normalizeFlag(item.failed) ? 'failed' : 'ok';
      return [
        ref || 'no-ref',
        status || 'unknown',
        terminal,
        failed
      ].join('/');
    })
    .sort();
  return buildCollectionSummary(entries);
};

const buildCollectionSummary = (entries: string[]): RenderShadowCollectionSummary => {
  const activeCount = entries.filter((entry) => {
    const parts = entry.split('/');
    return ACTIVE_COLLECTION_STATUSES.has(parts[1] || '');
  }).length;
  return {
    count: entries.length,
    activeCount,
    signature: entries.length > 0 ? entries.join('|') : EMPTY_COLLECTION_SIGNATURE,
    entries: entries.slice(0, 12)
  };
};

const normalizeRecordArray = (value: unknown): Record<string, unknown>[] =>
  Array.isArray(value) ? value.map(normalizeRecord).filter((item) => Object.keys(item).length > 0) : [];

const normalizeRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};

const buildRenderShadowFingerprint = (
  issues: ChatRuntimeRenderShadowIssue[]
): string => {
  if (issues.length === 0) return 'ok';
  return issues
    .slice(0, 12)
    .map((issue) => [
      issue.code,
      issue.role || '',
      issue.key || '',
      issue.legacyIndex ?? '',
      issue.projectionIndex ?? ''
    ].join(':'))
    .join('|');
};

const uniqueKeys = (values: string[]): string[] => {
  const seen = new Set<string>();
  const result: string[] = [];
  values.forEach((value) => {
    const normalized = String(value || '').trim();
    if (!normalized || seen.has(normalized)) return;
    seen.add(normalized);
    result.push(normalized);
  });
  return result;
};

const firstText = (...values: unknown[]): string => {
  for (const value of values) {
    const text = normalizeText(value);
    if (text) return text;
  }
  return '';
};

const normalizeRole = (value: unknown): RenderShadowRole | '' => {
  const role = normalizeText(value).toLowerCase();
  return role === 'user' || role === 'assistant' ? role : '';
};

const normalizeText = (value: unknown): string => String(value ?? '').trim();

const normalizeStatus = (value: unknown): string => normalizeText(value).toLowerCase();

const normalizeContent = (value: unknown): string =>
  String(value ?? '').replace(/\s+/g, ' ').trim();

const normalizeFlag = (value: unknown): boolean => {
  if (typeof value === 'string') {
    const text = value.trim().toLowerCase();
    if (!text) return false;
    return text !== 'false' && text !== '0' && text !== 'no';
  }
  return Boolean(value);
};
