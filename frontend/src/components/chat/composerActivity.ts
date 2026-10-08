import { shouldDisplayTransientRetry } from '@/utils/retryVisibility';

/**
 * 输入区上方悬浮状态条的数据推导（方案：状态行 + 计划行悬浮于输入区之上）。
 *
 * 数据全部来自既有消息流：活动回合的运行状态来自尾部 assistant 消息的
 * workflowItems / runtime_status（与 MessageWaitingNotice 同源），计划进度来自
 * 消息 plan 字段（update_plan 工具产生的 plan_update 已由 store 合并），
 * 文件变更统计来自 apply_patch 工具结果里的 changed_files / added_lines /
 * deleted_lines。这里只做轻量扫描，不复制大对象。
 */

export type ActivityPhase =
  | 'queued'
  | 'preparing'
  | 'model'
  | 'tool'
  | 'subagent'
  | 'compacting'
  | 'retrying';

export type ActivityStatus = {
  active: boolean;
  phase: ActivityPhase;
  labelKey: string;
  labelParams: Record<string, unknown> | null;
};

export type FileChangeStats = {
  files: number;
  addedLines: number;
  deletedLines: number;
};

export type PlanProgress = {
  total: number;
  done: number;
  current: string;
};

type WorkflowItemLike = {
  eventType?: unknown;
  event?: unknown;
  event_type?: unknown;
  isTool?: unknown;
  is_tool?: unknown;
  status?: unknown;
  state?: unknown;
  title?: unknown;
  detail?: unknown;
  toolName?: unknown;
  tool?: unknown;
  tool_name?: unknown;
  name?: unknown;
  toolDisplayName?: unknown;
  tool_display_name?: unknown;
  displayName?: unknown;
  display_name?: unknown;
  attempt?: unknown;
  retryReason?: unknown;
  delayS?: unknown;
  maxAttempts?: unknown;
};

type MessageLike = {
  role?: unknown;
  runtime_status?: unknown;
  runtimeStatus?: unknown;
  status?: unknown;
  state?: unknown;
  workflowStreaming?: unknown;
  stream_incomplete?: unknown;
  resume_available?: unknown;
  slow_client?: unknown;
  retry_started_at_ms?: unknown;
  workflowItems?: WorkflowItemLike[];
};

const TOOL_EVENT_PREFIXES = ['tool_', 'subagent_'];
const TOOL_EVENT_TYPES = new Set([
  'command_session_start',
  'command_session_status',
  'command_session_delta'
]);
// 结果类事件：最近一条工具事件落在这些类型上说明当前批次已收敛。
const TOOL_RESULT_EVENT_TYPES = new Set([
  'tool_result',
  'tool_call_completed',
  'tool_call_failed',
  'command_session_exit',
  'command_session_summary'
]);
const QUEUE_EVENT_TYPES = new Set(['queued', 'queue_enter', 'queue_update']);

const normalizeText = (value: unknown): string => String(value ?? '').trim().toLowerCase();
const normalizeFlag = (value: unknown): boolean => value === true || value === 'true';

/** 兼容 AgentRenderableMessage 包装与裸消息记录两种形态。 */
export const unwrapActivityMessage = (entry: unknown): MessageLike | null => {
  if (!entry || typeof entry !== 'object') return null;
  const record = entry as { message?: unknown };
  const message = record.message && typeof record.message === 'object' ? record.message : entry;
  return message as MessageLike;
};

const isAssistant = (message: MessageLike | null): boolean =>
  normalizeText(message?.role) === 'assistant';

const eventTypeOf = (item: WorkflowItemLike): string =>
  normalizeText(item.eventType ?? item.event ?? item.event_type);

const toolNameOf = (item: WorkflowItemLike): string =>
  normalizeText(item.toolName ?? item.tool ?? item.tool_name ?? item.name);

const toolLabelOf = (item: WorkflowItemLike): string =>
  String(item.toolDisplayName ?? item.tool_display_name ?? item.displayName ?? item.display_name ?? '')
    .trim() || toolNameOf(item);

const isToolishItem = (item: WorkflowItemLike): boolean => {
  const eventType = eventTypeOf(item);
  if (!eventType) return normalizeFlag(item.isTool ?? item.is_tool);
  if (TOOL_EVENT_TYPES.has(eventType)) return true;
  return TOOL_EVENT_PREFIXES.some((prefix) => eventType.startsWith(prefix));
};

const parseQueueAhead = (item: WorkflowItemLike | null): number | null => {
  if (!item) return null;
  let detail: Record<string, unknown> | null = null;
  if (typeof item.detail === 'string') {
    try {
      const parsed = JSON.parse(item.detail);
      if (parsed && typeof parsed === 'object') detail = parsed as Record<string, unknown>;
    } catch {
      detail = null;
    }
  } else if (item.detail && typeof item.detail === 'object') {
    detail = item.detail as Record<string, unknown>;
  }
  const candidates = [
    detail?.wait_ahead,
    detail?.waitAhead,
    detail?.queue_ahead,
    detail?.queueAhead,
    (detail?.data as Record<string, unknown> | undefined)?.wait_ahead,
    (detail?.data as Record<string, unknown> | undefined)?.waitAhead,
    (detail?.data as Record<string, unknown> | undefined)?.queue_ahead,
    (detail?.data as Record<string, unknown> | undefined)?.queueAhead
  ];
  for (const candidate of candidates) {
    const parsed = Number.parseInt(String(candidate ?? ''), 10);
    if (Number.isFinite(parsed) && parsed >= 0) return parsed;
  }
  return null;
};

const parseRetryAttempt = (item: WorkflowItemLike | null): number => {
  const parsed = Number.parseInt(String(item?.attempt ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
};

/**
 * 从尾部消息推导活动回合状态。loading 表示会话已发起但尾部 assistant
 * 尚未落任何事件（准备阶段）。等待用户确认（approval/inquiry）时状态条
 * 不与审批条争位，按空闲处理。
 */
export const deriveActivityStatus = (
  messages: readonly unknown[],
  loading: boolean
): ActivityStatus => {
  let tail: MessageLike | null = null;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = unwrapActivityMessage(messages[index]);
    if (isAssistant(message)) {
      tail = message;
      break;
    }
  }

  const runtimeStatus = normalizeText(
    tail?.runtime_status ?? tail?.runtimeStatus ?? tail?.status ?? tail?.state
  );
  if (runtimeStatus === 'waiting_input' || runtimeStatus === 'waiting_user_input') {
    return { active: false, phase: 'preparing', labelKey: 'chat.activity.preparing', labelParams: null };
  }

  const pending = Boolean(
    loading ||
    (tail && (normalizeFlag(tail.workflowStreaming) || normalizeFlag(tail.stream_incomplete) ||
      normalizeFlag(tail.resume_available) || normalizeFlag(tail.slow_client)))
  );
  if (!pending || !tail) {
    return { active: false, phase: 'preparing', labelKey: 'chat.activity.preparing', labelParams: null };
  }

  // 单次反向扫描：同时取最新的排队 / 重试 / 请求 / 工具 / 压缩事件。
  const items = Array.isArray(tail.workflowItems) ? tail.workflowItems : [];
  const scanLimit = Math.min(items.length, 80);
  let latestQueue: WorkflowItemLike | null = null;
  let latestQueueIndex = -1;
  let latestRequestIndex = -1;
  let latestRetry: WorkflowItemLike | null = null;
  let latestTool: WorkflowItemLike | null = null;
  let latestToolEventType = '';
  let latestCompactionIndex = -1;
  for (let index = items.length - 1; index >= items.length - scanLimit; index -= 1) {
    const item = items[index];
    const eventType = eventTypeOf(item);
    if (!latestTool && (isToolishItem(item) || TOOL_RESULT_EVENT_TYPES.has(eventType))) {
      latestTool = item;
      latestToolEventType = eventType;
    }
    if (latestQueueIndex < 0 && QUEUE_EVENT_TYPES.has(eventType)) {
      latestQueue = item;
      latestQueueIndex = index;
    }
    if (latestRequestIndex < 0 && eventType === 'llm_request') {
      latestRequestIndex = index;
    }
    if (!latestRetry && eventType === 'llm_stream_retry') {
      latestRetry = item;
    }
    if (latestCompactionIndex < 0 && (eventType === 'compaction' || eventType === 'compaction_progress')) {
      latestCompactionIndex = index;
    }
  }

  const retryVisible =
    Boolean(latestRetry) &&
    shouldDisplayTransientRetry(
      { retry_attempt: parseRetryAttempt(latestRetry), retry_started_at_ms: tail.retry_started_at_ms },
      Date.now()
    );
  if (retryVisible) {
    return { active: true, phase: 'retrying', labelKey: 'chat.activity.retrying', labelParams: null };
  }
  if (latestCompactionIndex >= 0 && latestCompactionIndex > latestRequestIndex) {
    return { active: true, phase: 'compacting', labelKey: 'chat.activity.compacting', labelParams: null };
  }
  const queued =
    runtimeStatus === 'queued' ||
    (latestQueueIndex >= 0 && latestQueueIndex > latestRequestIndex);
  if (queued) {
    const ahead = parseQueueAhead(latestQueue);
    return ahead !== null
      ? { active: true, phase: 'queued', labelKey: 'chat.activity.queuedAhead', labelParams: { count: ahead } }
      : { active: true, phase: 'queued', labelKey: 'chat.activity.queued', labelParams: null };
  }
  if (latestTool && !TOOL_RESULT_EVENT_TYPES.has(latestToolEventType)) {
    const label = toolLabelOf(latestTool);
    if (latestToolEventType.startsWith('subagent_') || toolNameOf(latestTool).startsWith('subagent')) {
      return { active: true, phase: 'subagent', labelKey: 'chat.activity.subagent', labelParams: null };
    }
    return label
      ? { active: true, phase: 'tool', labelKey: 'chat.activity.toolNamed', labelParams: { name: label } }
      : { active: true, phase: 'tool', labelKey: 'chat.activity.tool', labelParams: null };
  }
  if (latestRequestIndex >= 0) {
    return { active: true, phase: 'model', labelKey: 'chat.activity.model', labelParams: null };
  }
  return { active: true, phase: 'preparing', labelKey: 'chat.activity.preparing', labelParams: null };
};

const toInt = (value: unknown): number => {
  const parsed = Number.parseInt(String(value ?? ''), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
};

const readStatsObject = (parsed: Record<string, unknown>): FileChangeStats | null => {
  // apply_patch 结果可能是裸数据，也可能包在 result / data / result.data 里。
  const containers: Array<Record<string, unknown>> = [parsed];
  for (const key of ['result', 'data']) {
    const nested = parsed[key];
    if (nested && typeof nested === 'object' && !Array.isArray(nested)) {
      containers.push(nested as Record<string, unknown>);
      const deeper = (nested as Record<string, unknown>).data;
      if (deeper && typeof deeper === 'object' && !Array.isArray(deeper)) {
        containers.push(deeper as Record<string, unknown>);
      }
    }
  }
  for (const container of containers) {
    if (!('changed_files' in container) && !('added_lines' in container) && !('deleted_lines' in container)) {
      continue;
    }
    return {
      files: toInt(container.changed_files),
      addedLines: toInt(container.added_lines),
      deletedLines: toInt(container.deleted_lines)
    };
  }
  return null;
};

type CachedFileStats = { itemsRef: object; itemsLength: number; stats: FileChangeStats };

const activityStatsCache = new WeakMap<object, CachedFileStats>();

const messageFileStats = (message: MessageLike): FileChangeStats => {
  const items = Array.isArray(message.workflowItems) ? message.workflowItems : [];
  const stats: FileChangeStats = { files: 0, addedLines: 0, deletedLines: 0 };
  for (const item of items) {
    const detail = item?.detail;
    // 子串预筛避免对每条工具输出做 JSON.parse；超大 detail 直接跳过。
    if (typeof detail !== 'string' || detail.length > 262144) continue;
    if (!detail.includes('"changed_files"') && !detail.includes('"added_lines"')) continue;
    let parsed: unknown;
    try {
      parsed = JSON.parse(detail);
    } catch {
      continue;
    }
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) continue;
    const found = readStatsObject(parsed as Record<string, unknown>);
    if (!found) continue;
    stats.files += found.files;
    stats.addedLines += found.addedLines;
    stats.deletedLines += found.deletedLines;
  }
  return stats;
};

/**
 * 汇总最近若干条 assistant 消息的文件变更统计。旧回合的消息不再变动，
 * 用 WeakMap 按消息缓存；活动消息流式追加会使 workflowItems 引用或长度
 * 变化，此时缓存失效重算，保证计数实时。
 */
export const summarizeFileChanges = (messages: readonly unknown[]): FileChangeStats => {
  const stats: FileChangeStats = { files: 0, addedLines: 0, deletedLines: 0 };
  let scanned = 0;
  for (let index = messages.length - 1; index >= 0 && scanned < 24; index -= 1) {
    const message = unwrapActivityMessage(messages[index]);
    if (!isAssistant(message)) continue;
    scanned += 1;
    const items = message!.workflowItems;
    if (!items?.length) continue;
    const cached = activityStatsCache.get(message!);
    const valid = cached && cached.itemsRef === items && cached.itemsLength === items.length;
    const found = valid ? cached.stats : messageFileStats(message!);
    if (!valid) {
      activityStatsCache.set(message!, { itemsRef: items, itemsLength: items.length, stats: found });
    }
    stats.files += found.files;
    stats.addedLines += found.addedLines;
    stats.deletedLines += found.deletedLines;
  }
  return stats;
};

export const summarizePlanProgress = (plan: unknown): PlanProgress => {
  const steps = Array.isArray((plan as { steps?: unknown } | null)?.steps)
    ? ((plan as { steps: Array<{ step?: unknown; status?: unknown }> }).steps)
    : [];
  let done = 0;
  let current = '';
  for (const step of steps) {
    const status = normalizeText(step?.status);
    if (status === 'completed') {
      done += 1;
      continue;
    }
    if (!current && (status === 'in_progress' || status === 'pending')) {
      current = String(step?.step ?? '').trim();
    }
  }
  return { total: steps.length, done, current };
};
