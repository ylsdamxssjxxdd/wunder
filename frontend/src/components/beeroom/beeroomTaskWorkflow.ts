import type { BeeroomMissionTask } from '@/stores/beeroom';

export type BeeroomWorkflowTone = 'pending' | 'loading' | 'completed' | 'failed';

export type BeeroomWorkflowItem = {
  id: string;
  title: string;
  detail: string;
  status: BeeroomWorkflowTone;
  isTool?: boolean;
  eventType?: string;
  toolName?: string;
  toolCallId?: string;
};

export type BeeroomWorkflowPreviewStep = {
  key: string;
  title: string;
  detail: string;
  tone: BeeroomWorkflowTone;
};

export type BeeroomNodeWorkflowLine = {
  key: string;
  main: string;
  detail: string;
  title: string;
};

export type BeeroomTaskWorkflowPreview = {
  badge: string;
  badgeTone: BeeroomWorkflowTone;
  steps: BeeroomWorkflowPreviewStep[];
  fingerprint: string;
};

type SessionEvent = {
  event?: string;
  data?: unknown;
  timestamp?: string;
};

type SessionRound = {
  events?: SessionEvent[];
};

type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

const PREVIEW_STEP_LIMIT = 1;
const PREVIEW_TITLE_LIMIT = 22;
const PREVIEW_DETAIL_LIMIT = 42;
const NODE_WORKFLOW_TOOL_LIMIT = 6;
const NODE_WORKFLOW_EVENT_TITLE_LIMIT = 12;
const NODE_WORKFLOW_DETAIL_LIMIT = 10;
const MIN_WINDOW_PADDING_S = 2;
const ACTIVE_WINDOW_FALLBACK_S = 30;
const TERMINAL_WINDOW_FALLBACK_S = 60;
const ACTIVE_TASK_SELECTION_STATUSES = new Set(['queued', 'pending', 'running', 'awaiting_idle', 'merging']);

const TERMINAL_STATUSES = new Set(['success', 'completed', 'failed', 'error', 'timeout', 'cancelled']);

const asRecord = (value: unknown): Record<string, unknown> | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return null;
  }
  return value as Record<string, unknown>;
};

const normalizeText = (value: unknown): string => String(value || '').trim();

const normalizeStatus = (value: unknown): string => normalizeText(value).toLowerCase();

const resolveTaskSessionIdentity = (task: BeeroomMissionTask | null | undefined): string =>
  normalizeText(task?.spawned_session_id || task?.target_session_id || task?.session_run_id);

export const resolveBeeroomTaskMoment = (task: BeeroomMissionTask | null | undefined): number =>
  Number(task?.updated_time || task?.finished_time || task?.started_time || 0);

export const isBeeroomTaskStatusActive = (value: unknown): boolean =>
  ACTIVE_TASK_SELECTION_STATUSES.has(normalizeStatus(value));

export const compareBeeroomMissionTasksByDisplayPriority = (
  left: BeeroomMissionTask | null | undefined,
  right: BeeroomMissionTask | null | undefined
) => {
  const activeDiff = Number(isBeeroomTaskStatusActive(right?.status)) - Number(isBeeroomTaskStatusActive(left?.status));
  if (activeDiff !== 0) return activeDiff;
  const timeDiff = resolveBeeroomTaskMoment(right) - resolveBeeroomTaskMoment(left);
  if (timeDiff !== 0) return timeDiff;
  const sessionDiff = Number(Boolean(resolveTaskSessionIdentity(right))) - Number(Boolean(resolveTaskSessionIdentity(left)));
  if (sessionDiff !== 0) return sessionDiff;
  return normalizeText(left?.task_id).localeCompare(normalizeText(right?.task_id), 'zh-Hans-CN');
};

const escapeHtml = (value: unknown): string =>
  String(value || '')
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');

const truncateSingleLine = (value: unknown, limit: number): string => {
  const text = normalizeText(value).replace(/\s+/g, ' ');
  if (!text) return '';
  if (text.length <= limit) return text;
  return `${text.slice(0, Math.max(0, limit - 3))}...`;
};

const truncateMiddleSingleLine = (value: unknown, limit: number): string => {
  const text = normalizeText(value).replace(/\s+/g, ' ');
  if (!text) return '';
  if (text.length <= limit) return text;
  const bodyLimit = Math.max(limit - 3, 2);
  const head = Math.max(1, Math.ceil(bodyLimit * 0.6));
  const tail = Math.max(1, bodyLimit - head);
  return `${text.slice(0, head)}...${text.slice(text.length - tail)}`;
};

const parseIsoTimestamp = (value: unknown): number => {
  const text = normalizeText(value);
  if (!text) return 0;
  const millis = Date.parse(text);
  return Number.isFinite(millis) ? millis / 1000 : 0;
};

const isTerminalStatus = (value: unknown): boolean => TERMINAL_STATUSES.has(normalizeStatus(value));

const resolveWorkflowTone = (value: unknown): BeeroomWorkflowTone => {
  const status = normalizeStatus(value);
  if (status === 'failed' || status === 'error' || status === 'timeout' || status === 'cancelled') {
    return 'failed';
  }
  if (status === 'running' || status === 'loading' || status === 'executing' || status === 'merging') {
    return 'loading';
  }
  if (status === 'queued' || status === 'pending' || status === 'awaiting_idle') {
    return 'pending';
  }
  return 'completed';
};

const resolveBeeroomStatusLabel = (status: unknown, t: TranslationFn): string => {
  const normalized = normalizeStatus(status);
  if (normalized === 'queued') return t('beeroom.status.queued');
  if (normalized === 'running') return t('beeroom.status.running');
  if (normalized === 'awaiting_idle') return t('beeroom.status.awaitingIdle');
  if (normalized === 'completed' || normalized === 'success') return t('beeroom.status.completed');
  if (normalized === 'failed' || normalized === 'error') return t('beeroom.status.failed');
  if (normalized === 'timeout') return t('beeroom.status.timeout');
  if (normalized === 'cancelled') return t('beeroom.status.cancelled');
  return t('beeroom.status.unknown');
};

const buildDetail = (value: unknown): string => {
  if (typeof value === 'string') return value.trim();
  if (value === null || value === undefined) return '';
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
};

const resolveToolName = (payload: Record<string, unknown>): string =>
  normalizeText(
    payload.tool ?? payload.name ?? payload.tool_name ?? payload.toolName ?? payload.command_name
  );

const resolveToolCallId = (payload: Record<string, unknown>): string =>
  normalizeText(
    payload.tool_call_id ?? payload.toolCallId ?? payload.call_id ?? payload.callId ?? payload.id
  );

const resolveEventDetail = (eventName: string, payload: Record<string, unknown>, t: TranslationFn): string => {
  const summary = normalizeText(
    payload.result_summary ?? payload.summary ?? payload.message ?? payload.error ?? payload.detail
  );
  if (summary) return summary;
  const stage = normalizeText(payload.stage);
  if (eventName === 'progress' && stage) {
    return t('chat.workflow.stage', { stage });
  }
  const command = normalizeText(payload.command);
  if (command) return command;
  const args = payload.args ?? payload.arguments ?? payload.input;
  const argsText = normalizeText(typeof args === 'string' ? args : buildDetail(args));
  if (argsText) return argsText;
  const content = normalizeText(payload.content ?? payload.answer ?? payload.output);
  if (content) return content;
  return '';
};

const buildFallbackWorkflowItems = (task: BeeroomMissionTask | null, t: TranslationFn): BeeroomWorkflowItem[] => {
  if (!task) {
    return [
      {
        id: 'standby',
        title: t('chat.toolWorkflow.empty'),
        detail: '',
        status: 'pending'
      }
    ];
  }
  const detail = normalizeText(task.result_summary || task.error || '');
  return [
    {
      id: `${task.task_id || 'task'}:status`,
      title: resolveBeeroomStatusLabel(task.status, t),
      detail,
      status: resolveWorkflowTone(task.status)
    }
  ];
};

const flattenWorkflowEvents = (rounds: SessionRound[]): SessionEvent[] => {
  const events: SessionEvent[] = [];
  rounds.forEach((round) => {
    if (!Array.isArray(round?.events)) return;
    round.events.forEach((event) => {
      if (!event || typeof event !== 'object') return;
      events.push(event);
    });
  });
  return events;
};

const filterTaskWindowEvents = (task: BeeroomMissionTask, events: SessionEvent[]): SessionEvent[] => {
  const startedAt = Number(task.started_time || 0);
  const finishedAt = Number(task.finished_time || 0);
  const updatedAt = Number(task.updated_time || 0);
  const fallbackWindow = isTerminalStatus(task.status)
    ? TERMINAL_WINDOW_FALLBACK_S
    : ACTIVE_WINDOW_FALLBACK_S;
  const windowStart = Math.max(0, (startedAt || updatedAt || 0) - Math.max(MIN_WINDOW_PADDING_S, fallbackWindow));
  const windowEnd = finishedAt > 0 ? finishedAt + MIN_WINDOW_PADDING_S : Number.POSITIVE_INFINITY;

  return events.filter((event) => {
    const timestamp = parseIsoTimestamp(event.timestamp);
    if (!timestamp) {
      return windowStart <= 0;
    }
    return timestamp >= windowStart && timestamp <= windowEnd;
  });
};

const mapSessionEventToWorkflowItem = (
  event: SessionEvent,
  index: number,
  t: TranslationFn
): BeeroomWorkflowItem | null => {
  const eventName = normalizeText(event?.event).toLowerCase();
  if (!eventName || eventName === 'heartbeat' || eventName === 'ping') {
    return null;
  }
  const payload = asRecord(event?.data) || {};
  const toolName = resolveToolName(payload);
  const toolCallId = resolveToolCallId(payload);
  const detail = buildDetail(payload);

  if (eventName === 'tool_call') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.toolCall', { tool: toolName || t('chat.workflow.toolUnknown') }),
      detail,
      status: 'loading',
      isTool: true,
      eventType: 'tool_call',
      toolName,
      toolCallId: toolCallId || undefined
    };
  }
  if (eventName === 'tool_result') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.toolResult', { tool: toolName || t('chat.workflow.toolUnknown') }),
      detail,
      status: normalizeText(payload.error) ? 'failed' : 'completed',
      isTool: true,
      eventType: 'tool_result',
      toolName,
      toolCallId: toolCallId || undefined
    };
  }
  if (eventName === 'progress') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.progressUpdate'),
      detail,
      status: 'loading',
      eventType: eventName
    };
  }
  if (eventName === 'llm_request') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.modelRequestSummary'),
      detail,
      status: 'loading',
      eventType: eventName
    };
  }
  if (eventName === 'llm_output') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.modelOutput'),
      detail,
      status: 'completed',
      eventType: eventName
    };
  }
  if (eventName === 'final') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.finalResponse'),
      detail,
      status: 'completed',
      eventType: eventName
    };
  }
  if (eventName === 'error') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('chat.workflow.error'),
      detail,
      status: 'failed',
      eventType: eventName
    };
  }
  if (eventName === 'team_task_dispatch') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('beeroom.canvas.legendDispatch'),
      detail,
      status: 'pending',
      eventType: eventName
    };
  }
  if (eventName === 'team_task_update' || eventName === 'team_task_result') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: resolveBeeroomStatusLabel(payload.status, t),
      detail,
      status: resolveWorkflowTone(payload.status),
      eventType: eventName
    };
  }
  if (eventName === 'team_error') {
    return {
      id: `workflow:${eventName}:${index}`,
      title: t('beeroom.status.failed'),
      detail,
      status: 'failed',
      eventType: eventName
    };
  }
  return {
    id: `workflow:${eventName}:${index}`,
    title: t('chat.workflow.event', { event: eventName }),
    detail,
    status: 'completed',
    eventType: eventName
  };
};

const dedupePreviewSteps = (steps: BeeroomWorkflowPreviewStep[]): BeeroomWorkflowPreviewStep[] => {
  const output: BeeroomWorkflowPreviewStep[] = [];
  steps.forEach((step) => {
    const previous = output[output.length - 1];
    if (previous && previous.title === step.title && previous.detail === step.detail && previous.tone === step.tone) {
      return;
    }
    output.push(step);
  });
  return output;
};

// Parse preview details only when they look like JSON so card rendering stays cheap.
const parsePreviewDetailRecord = (detail: string): Record<string, unknown> | null => {
  const text = normalizeText(detail);
  if (!text) return null;
  try {
    return asRecord(JSON.parse(text));
  } catch {
    return null;
  }
};

function stringifyWorkflowArgValue(value: unknown): string {
  if (typeof value === 'string') return truncateSingleLine(value, 18);
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  if (Array.isArray(value)) {
    return truncateSingleLine(value.map((entry) => stringifyWorkflowArgValue(entry)).filter(Boolean).join(', '), 18);
  }
  if (value && typeof value === 'object') {
    try {
      return truncateSingleLine(JSON.stringify(value), 18);
    } catch {
      return '';
    }
  }
  return '';
}

const normalizeWorkflowToolLookup = (value: unknown): string =>
  normalizeText(value).toLowerCase().replace(/\s+/g, '_');

const isSkillWorkflowTool = (normalizedTool: string, rawTool: string): boolean =>
  normalizedTool.includes('skill') || rawTool.includes('技能');

const isSearchWorkflowTool = (normalizedTool: string, rawTool: string): boolean =>
  normalizedTool.includes('knowledge') ||
  normalizedTool.includes('retriev') ||
  normalizedTool.includes('query') ||
  normalizedTool.includes('search_content') ||
  normalizedTool.includes('search') ||
  normalizedTool.includes('grep') ||
  rawTool.includes('知识库') ||
  rawTool.includes('检索') ||
  rawTool.includes('搜索内容');

const isCommandWorkflowTool = (normalizedTool: string, rawTool: string): boolean =>
  normalizedTool.includes('command') ||
  normalizedTool.includes('shell') ||
  normalizedTool.includes('terminal') ||
  normalizedTool.includes('exec') ||
  rawTool.includes('执行命令');

const isBrowserWorkflowTool = (normalizedTool: string, rawTool: string): boolean =>
  normalizedTool.includes('open_url') ||
  normalizedTool.includes('browser') ||
  normalizedTool.includes('fetch') ||
  normalizedTool.includes('web') ||
  rawTool.includes('网页') ||
  rawTool.includes('浏览');

const resolveCompactWorkflowToolLabel = (value: unknown): string => {
  const raw = normalizeText(value);
  const normalized = normalizeWorkflowToolLookup(value);
  if (!raw) return '';
  if (isSkillWorkflowTool(normalized, raw)) return '技能';
  if (normalized.includes('read_file') || raw.includes('读取文件')) return '读取';
  if (normalized.includes('list_files') || normalized.includes('list_dir') || raw.includes('列出文件')) {
    return '列出';
  }
  if (isSearchWorkflowTool(normalized, raw)) {
    return '检索';
  }
  if (normalized.includes('write_file') || raw.includes('写入文件')) return '写入';
  if (normalized.includes('edit_file') || normalized.includes('patch') || raw.includes('编辑文件')) {
    return '编辑';
  }
  if (normalized.includes('delete_file') || normalized.includes('remove') || raw.includes('删除文件')) {
    return '删除';
  }
  if (isCommandWorkflowTool(normalized, raw)) {
    return '执行';
  }
  if (isBrowserWorkflowTool(normalized, raw)) {
    return '访问';
  }
  const stripped = raw
    .replace(/[_-]+/g, ' ')
    .replace(/(工具|技能|能力|服务|插件)$/u, '')
    .replace(/\b(tool|skill|ability|service|plugin)\b/gi, '')
    .trim();
  if (!stripped) {
    return truncateSingleLine(raw, NODE_WORKFLOW_TOOL_LIMIT);
  }
  const tokenized = stripped.split(/[/:.\s]+/).filter(Boolean);
  const fallback = tokenized[tokenized.length - 1] || stripped;
  return truncateSingleLine(fallback, NODE_WORKFLOW_TOOL_LIMIT);
};

const extractWorkflowScalar = (value: unknown): string => {
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return normalizeText(value);
  }
  if (Array.isArray(value)) {
    for (const entry of value) {
      const text = extractWorkflowScalar(entry);
      if (text) return text;
    }
    return '';
  }
  if (value && typeof value === 'object') {
    const record = value as Record<string, unknown>;
    return extractWorkflowScalar(
      record.path ??
        record.file ??
        record.file_path ??
        record.name ??
        record.query ??
        record.prompt ??
        record.url ??
        record.command ??
        record.message ??
        record.text
    );
  }
  return '';
};

const pickWorkflowValue = (
  record: Record<string, unknown> | null,
  keys: string[]
): unknown => {
  if (!record) return null;
  for (const key of keys) {
    if (record[key] !== undefined && record[key] !== null) {
      return record[key];
    }
  }
  return null;
};

const resolveWorkflowArgRecord = (payload: Record<string, unknown>): Record<string, unknown> | null =>
  asRecord(payload.args ?? payload.arguments ?? payload.input ?? payload.params ?? payload.parameters ?? null);

const shortenWorkflowPath = (value: unknown): string => {
  const text = extractWorkflowScalar(value).replace(/\\/g, '/');
  if (!text) return '';
  const parts = text.split('/').filter(Boolean);
  const tail = parts[parts.length - 1] || text;
  return truncateMiddleSingleLine(tail, NODE_WORKFLOW_DETAIL_LIMIT);
};

const shortenWorkflowQuery = (value: unknown): string =>
  truncateSingleLine(extractWorkflowScalar(value), NODE_WORKFLOW_DETAIL_LIMIT);

const shortenWorkflowCommand = (value: unknown): string => {
  const text = extractWorkflowScalar(value);
  if (!text) return '';
  const tokens = text.split(/\s+/).filter(Boolean);
  if (tokens.length >= 2) {
    return truncateSingleLine(`${tokens[0]} ${tokens[1]}`, NODE_WORKFLOW_DETAIL_LIMIT);
  }
  return truncateSingleLine(text, NODE_WORKFLOW_DETAIL_LIMIT);
};

const shortenWorkflowUrl = (value: unknown): string => {
  const text = extractWorkflowScalar(value);
  if (!text) return '';
  try {
    const url = new URL(text);
    return truncateMiddleSingleLine(url.hostname || url.pathname || text, NODE_WORKFLOW_DETAIL_LIMIT);
  } catch {
    return truncateMiddleSingleLine(text, NODE_WORKFLOW_DETAIL_LIMIT);
  }
};

const resolveGenericWorkflowDetail = (
  payload: Record<string, unknown>,
  argRecord: Record<string, unknown> | null
): string => {
  const preferred = [
    pickWorkflowValue(argRecord, ['path', 'file', 'file_path', 'filepath', 'target', 'targets']),
    pickWorkflowValue(argRecord, ['directory', 'dir', 'folder', 'cwd']),
    pickWorkflowValue(argRecord, ['query', 'keyword', 'keywords', 'question', 'prompt', 'text']),
    pickWorkflowValue(argRecord, ['command', 'cmd', 'script']),
    pickWorkflowValue(argRecord, ['url', 'uri', 'link']),
    pickWorkflowValue(argRecord, ['name', 'title', 'task', 'message']),
    payload.path,
    payload.file,
    payload.query,
    payload.prompt,
    payload.command,
    payload.message,
    payload.summary
  ];
  for (const candidate of preferred) {
    const text = extractWorkflowScalar(candidate);
    if (text) {
      if (/[\\/]/.test(text)) {
        return shortenWorkflowPath(text);
      }
      return truncateSingleLine(text, NODE_WORKFLOW_DETAIL_LIMIT);
    }
  }
  return '';
};

const resolveWorkflowDetailPlaceholder = (normalizedTool: string, rawTool: string): string => {
  if (isSkillWorkflowTool(normalizedTool, rawTool)) return '调用';
  if (normalizedTool.includes('read_file') || normalizedTool.includes('write_file') || normalizedTool.includes('edit_file') || normalizedTool.includes('delete_file')) {
    return '文件';
  }
  if (normalizedTool.includes('list_files') || normalizedTool.includes('list_dir')) {
    return '目录';
  }
  if (isSearchWorkflowTool(normalizedTool, rawTool)) return '查询';
  if (isCommandWorkflowTool(normalizedTool, rawTool)) return '命令';
  if (isBrowserWorkflowTool(normalizedTool, rawTool)) return '链接';
  return '';
};

const resolveWorkflowHoverLines = (
  payload: Record<string, unknown> | null,
  toolName: string
): string[] => {
  const rawTool = normalizeText(toolName);
  const normalizedTool = normalizeWorkflowToolLookup(toolName);
  const lines: string[] = [];
  const argRecord = payload ? resolveWorkflowArgRecord(payload) : null;

  const appendLine = (label: string, value: unknown) => {
    const text = normalizeText(value);
    if (!text) return;
    const line = `${label}：${text}`;
    if (!lines.includes(line)) {
      lines.push(line);
    }
  };

  if (rawTool) {
    appendLine('工具', rawTool);
  }

  if (isSkillWorkflowTool(normalizedTool, rawTool)) {
    appendLine(
      '调用',
      extractWorkflowScalar(
        pickWorkflowValue(argRecord, ['skill', 'skill_name', 'skillName', 'runtime_name', 'runtimeName', 'name', 'title']) ??
          payload?.skill ??
          payload?.skill_name ??
          payload?.runtime_name ??
          payload?.name
      )
    );
  }

  if (
    normalizedTool.includes('read_file') ||
    normalizedTool.includes('write_file') ||
    normalizedTool.includes('edit_file') ||
    normalizedTool.includes('delete_file')
  ) {
    appendLine(
      '文件',
      extractWorkflowScalar(
        pickWorkflowValue(argRecord, ['path', 'file', 'file_path', 'filepath', 'target', 'targets']) ??
          pickWorkflowValue(argRecord, ['destination', 'dest']) ??
          payload?.path ??
          payload?.file
      )
    );
  }

  if (normalizedTool.includes('list_files') || normalizedTool.includes('list_dir')) {
    appendLine(
      '目录',
      extractWorkflowScalar(
        pickWorkflowValue(argRecord, ['path', 'directory', 'dir', 'folder', 'cwd', 'target']) ??
          payload?.path
      )
    );
  }

  if (isSearchWorkflowTool(normalizedTool, rawTool)) {
    appendLine(
      '查询',
      extractWorkflowScalar(
        pickWorkflowValue(argRecord, ['query', 'keyword', 'keywords', 'question', 'prompt', 'text']) ??
          payload?.query ??
          payload?.prompt ??
          payload?.message
      )
    );
  }

  if (isCommandWorkflowTool(normalizedTool, rawTool)) {
    appendLine(
      '命令',
      extractWorkflowScalar(
        pickWorkflowValue(argRecord, ['command', 'cmd', 'script']) ?? payload?.command
      )
    );
  }

  if (isBrowserWorkflowTool(normalizedTool, rawTool)) {
    appendLine(
      '链接',
      extractWorkflowScalar(
        pickWorkflowValue(argRecord, ['url', 'uri', 'link']) ?? payload?.url
      )
    );
  }

  if (!lines.length || lines.length === 1) {
    appendLine(
      '详情',
      extractWorkflowScalar(
        pickWorkflowValue(argRecord, ['name', 'title', 'task', 'message', 'summary', 'text']) ??
          payload?.summary ??
          payload?.message ??
          payload?.detail ??
          payload?.error
      )
    );
  }

  if (!lines.length || lines.length === 1) {
    const fallback = payload
      ? normalizeText(JSON.stringify(payload))
      : '';
    appendLine('详情', truncateSingleLine(fallback, 180));
  }

  if (lines.length === 1) {
    appendLine('详情', resolveWorkflowDetailPlaceholder(normalizedTool, rawTool));
  }

  return lines.slice(0, 4);
};

function summarizeWorkflowArgs(payload: Record<string, unknown> | null, toolName = ''): string {
  if (!payload) return '';
  const argRecord = resolveWorkflowArgRecord(payload);
  const argSource = payload.args ?? payload.arguments ?? payload.input ?? payload.params ?? payload.parameters ?? null;
  const rawTool = normalizeText(toolName || payload.toolName || payload.tool || payload.name);
  const normalizedTool = normalizeWorkflowToolLookup(rawTool);

  if (
    normalizedTool.includes('read_file') ||
    normalizedTool.includes('write_file') ||
    normalizedTool.includes('edit_file') ||
    normalizedTool.includes('delete_file')
  ) {
    const fileTarget =
      pickWorkflowValue(argRecord, ['path', 'file', 'file_path', 'filepath', 'target', 'targets']) ??
      pickWorkflowValue(argRecord, ['destination', 'dest']) ??
      payload.path ??
      payload.file;
    const text = shortenWorkflowPath(fileTarget);
    if (text) return text;
  }

  if (normalizedTool.includes('list_files') || normalizedTool.includes('list_dir')) {
    const dirTarget =
      pickWorkflowValue(argRecord, ['path', 'directory', 'dir', 'folder', 'cwd', 'target']) ??
      payload.path;
    const text = shortenWorkflowPath(dirTarget);
    if (text) return text;
  }

  if (
    isSearchWorkflowTool(normalizedTool, rawTool)
  ) {
    const queryTarget =
      pickWorkflowValue(argRecord, ['query', 'keyword', 'keywords', 'question', 'prompt', 'text']) ??
      payload.query ??
      payload.prompt ??
      payload.message;
    const text = shortenWorkflowQuery(queryTarget);
    if (text) return text;
  }

  if (
    isCommandWorkflowTool(normalizedTool, rawTool)
  ) {
    const commandTarget =
      pickWorkflowValue(argRecord, ['command', 'cmd', 'script']) ?? payload.command;
    const text = shortenWorkflowCommand(commandTarget);
    if (text) return text;
  }

  if (
    isBrowserWorkflowTool(normalizedTool, rawTool)
  ) {
    const urlTarget =
      pickWorkflowValue(argRecord, ['url', 'uri', 'link']) ?? payload.url;
    const text = shortenWorkflowUrl(urlTarget);
    if (text) return text;
  }

  if (typeof argSource === 'string') {
    return truncateSingleLine(argSource, NODE_WORKFLOW_DETAIL_LIMIT);
  }
  if (Array.isArray(argSource)) {
    const text = extractWorkflowScalar(argSource);
    if (text) {
      return /[\\/]/.test(text)
        ? shortenWorkflowPath(text)
        : truncateSingleLine(text, NODE_WORKFLOW_DETAIL_LIMIT);
    }
  }
  if (argSource && typeof argSource === 'object') {
    const detail = resolveGenericWorkflowDetail(payload, argRecord);
    if (detail) {
      return detail;
    }
  }
  const fallback = normalizeText(
    payload.command ?? payload.path ?? payload.file ?? payload.query ?? payload.prompt ?? payload.message ?? payload.summary
  );
  const resolvedFallback = /[\\/]/.test(fallback)
    ? shortenWorkflowPath(fallback)
    : truncateSingleLine(fallback, NODE_WORKFLOW_DETAIL_LIMIT);
  return resolvedFallback || resolveWorkflowDetailPlaceholder(normalizedTool, rawTool);
}

function resolveNodeWorkflowLineParts(item: BeeroomWorkflowItem): { main: string; detail: string } {
  const payload = parsePreviewDetailRecord(item.detail);
  const tool = resolveCompactWorkflowToolLabel(item.toolName || item.title || '') ||
    truncateSingleLine(item.title, NODE_WORKFLOW_TOOL_LIMIT);
  const detail = summarizeWorkflowArgs(payload, item.toolName || item.title || '');
  return {
    main: tool,
    detail: truncateMiddleSingleLine(detail, NODE_WORKFLOW_DETAIL_LIMIT)
  };
}

function resolveNodeWorkflowLineText(item: BeeroomWorkflowItem): string {
  const parts = resolveNodeWorkflowLineParts(item);
  return truncateSingleLine(parts.detail ? `${parts.main} · ${parts.detail}` : parts.main, 56);
}

function resolveNodeWorkflowHoverText(item: BeeroomWorkflowItem): string {
  const payload = parsePreviewDetailRecord(item.detail);
  const lines = resolveWorkflowHoverLines(payload, item.toolName || item.title || '');
  if (lines.length) {
    return lines.join('\n');
  }
  return resolveNodeWorkflowLineText(item);
}

function resolveNodeWorkflowEventLineParts(item: BeeroomWorkflowItem): { main: string; detail: string } {
  const payload = parsePreviewDetailRecord(item.detail);
  const argRecord = payload ? resolveWorkflowArgRecord(payload) : null;
  const summary = payload
    ? resolveGenericWorkflowDetail(payload, argRecord) ||
      extractWorkflowScalar(
        payload.summary ??
          payload.result_summary ??
          payload.message ??
          payload.detail ??
          payload.error ??
          payload.content ??
          payload.answer
      )
    : normalizeText(item.detail);
  return {
    main: truncateSingleLine(item.title || item.eventType || '事件', NODE_WORKFLOW_EVENT_TITLE_LIMIT),
    detail: truncateMiddleSingleLine(summary, NODE_WORKFLOW_DETAIL_LIMIT)
  };
}

function resolveNodeWorkflowEventHoverText(item: BeeroomWorkflowItem): string {
  const title = normalizeText(item.title || item.eventType || '');
  const detail = normalizeText(item.detail);
  if (title && detail) {
    return `${title}\n${detail}`;
  }
  return detail || title || resolveNodeWorkflowLineText(item);
}

const buildPreviewFromItems = (
  task: BeeroomMissionTask | null,
  items: BeeroomWorkflowItem[],
  t: TranslationFn
): BeeroomTaskWorkflowPreview => {
  const source = items.length > 0 ? items : buildFallbackWorkflowItems(task, t);
  const steps = dedupePreviewSteps(
    source
      .map((item, index) => {
        const parsedDetail = parsePreviewDetailRecord(item.detail);
        const detail = truncateSingleLine(
          resolveEventDetail(item.eventType || '', parsedDetail || {}, t) || item.detail,
          PREVIEW_DETAIL_LIMIT
        );
        return {
          key: `${item.id}:${index}`,
          title: truncateSingleLine(item.title, PREVIEW_TITLE_LIMIT),
          detail,
          tone: item.status
        } satisfies BeeroomWorkflowPreviewStep;
      })
      .filter((step) => step.title)
  )
    .slice(-PREVIEW_STEP_LIMIT)
    .reverse();
  const badge = task ? resolveBeeroomStatusLabel(task.status, t) : t('chat.toolWorkflow.empty');
  const badgeTone = task ? resolveWorkflowTone(task.status) : 'pending';
  const fingerprint = [
    badge,
    badgeTone,
    ...(task
      ? [
          task.task_id,
          normalizeText(task.status),
          normalizeText(task.updated_time),
          normalizeText(task.started_time),
          normalizeText(task.finished_time),
          normalizeText(task.result_summary),
          normalizeText(task.error)
        ]
      : ['standby']),
    ...steps.map((step) => `${step.title}:${step.detail}:${step.tone}`)
  ].join('|');
  return {
    badge,
    badgeTone,
    steps,
    fingerprint
  };
};

export const buildTaskWorkflowRuntime = (
  task: BeeroomMissionTask | null,
  rounds: SessionRound[] | null | undefined,
  t: TranslationFn
): { items: BeeroomWorkflowItem[]; preview: BeeroomTaskWorkflowPreview } => {
  if (!task) {
    const items = buildFallbackWorkflowItems(null, t);
    return {
      items,
      preview: buildPreviewFromItems(null, items, t)
    };
  }
  const flattenedEvents = flattenWorkflowEvents(Array.isArray(rounds) ? rounds : []);
  const scopedEvents = filterTaskWindowEvents(task, flattenedEvents);
  const items = scopedEvents
    .map((event, index) => mapSessionEventToWorkflowItem(event, index, t))
    .filter((item): item is BeeroomWorkflowItem => Boolean(item));

  if (!items.length) {
    const fallbackItems = buildFallbackWorkflowItems(task, t);
    return {
      items: fallbackItems,
      preview: buildPreviewFromItems(task, fallbackItems, t)
    };
  }

  return {
    items,
    preview: buildPreviewFromItems(task, items, t)
  };
};

export const buildSessionWorkflowItems = (
  rounds: SessionRound[] | null | undefined,
  t: TranslationFn
): BeeroomWorkflowItem[] =>
  flattenWorkflowEvents(Array.isArray(rounds) ? rounds : [])
    .map((event, index) => mapSessionEventToWorkflowItem(event, index, t))
    .filter((item): item is BeeroomWorkflowItem => Boolean(item));

const isNodeWorkflowToolItem = (item: BeeroomWorkflowItem): boolean =>
  item.isTool === true || item.eventType === 'tool_call' || item.eventType === 'tool_result';

export const buildNodeWorkflowHtml = (
  items: BeeroomWorkflowItem[],
  title: string,
  tone: BeeroomWorkflowTone = 'pending'
): string => {
  const toolItems = items.filter((item) => isNodeWorkflowToolItem(item));
  const lines = toolItems.length
    ? toolItems
        .slice()
        .map((item) => {
          const parts = resolveNodeWorkflowLineParts(item);
          return `
            <div class="beeroom-node-workflow-step" title="${escapeHtml(resolveNodeWorkflowHoverText(item))}">
              <span class="beeroom-node-workflow-step-dot"></span>
              <span class="beeroom-node-workflow-step-text">
                <span class="beeroom-node-workflow-step-main">${escapeHtml(parts.main)}</span>
                ${parts.detail ? `<span class="beeroom-node-workflow-step-detail">${escapeHtml(parts.detail)}</span>` : ''}
              </span>
            </div>
          `;
        })
        .join('')
    : `<div class="beeroom-node-workflow-empty">${escapeHtml(title)}</div>`;
  return `
    <div class="beeroom-node-workflow is-${escapeHtml(tone)} ${toolItems.length ? '' : 'is-empty'}">
      <div class="beeroom-node-workflow-steps">${lines}</div>
    </div>
  `;
};

export const buildNodeWorkflowPreviewLines = (
  items: BeeroomWorkflowItem[],
  options: { includeEventFallback?: boolean } = {}
): BeeroomNodeWorkflowLine[] => {
  const toolItems = items.filter((item) => isNodeWorkflowToolItem(item));
  const fallbackItems = items.filter((item) => item.eventType !== 'llm_request');
  const source =
    toolItems.length > 0
      ? toolItems
      : options.includeEventFallback
        ? (fallbackItems.length ? fallbackItems : items)
        : [];

  return source.slice().map((item, index) => {
    const isToolLine = isNodeWorkflowToolItem(item);
    const parts = isToolLine ? resolveNodeWorkflowLineParts(item) : resolveNodeWorkflowEventLineParts(item);
    return {
      key: `${item.id || item.eventType || 'workflow'}:${index}`,
      main: parts.main,
      detail: parts.detail,
      title: isToolLine ? resolveNodeWorkflowHoverText(item) : resolveNodeWorkflowEventHoverText(item)
    } satisfies BeeroomNodeWorkflowLine;
  });
};

