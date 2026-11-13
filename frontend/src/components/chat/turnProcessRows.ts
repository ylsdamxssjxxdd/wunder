/**
 * 工作过程懒加载条目的纯装配逻辑。
 *
 * `/thread-log/turns/{turn}` 返回的是持久条目（tool_call / compaction /
 * assistant_message …），这一层把它们折成工具条目与压缩条目，供
 * `MessageTimelineTurnProcess` 的折叠列表渲染。抽成独立模块有两个原因：
 * 装配规则必须与桌面端同口径（见 `crates/wunder-desktop/native_chat_process.rs`），
 * 并且要能用真实条目载荷做回归测试，而不是只能靠界面目测。
 */
import type { WorkflowItem } from './toolWorkflowRunModel';

export const TERMINAL_TOOL_STATUS = new Set(['completed', 'failed', 'cancelled', 'interrupted']);
const SKIP_KINDS = new Set(['user_message', 'tool_message']);

/**
 * 上下文压缩的持久条目 kind。运行时把自动压缩、循环压缩、手动压缩都写成
 * 独立 `compaction` 记录（payload 里只有 summary_text / event_type 等字段，
 * 没有 tool 字段）；旧的替代式压缩另有 `compacted` / `compaction_summary`。
 */
const COMPACTION_ITEM_KINDS = new Set(['compaction', 'compacted', 'compaction_summary']);
/** 压缩条目在工作过程里的工具身份：与运行时 reducer、桌面端同一口径。 */
export const COMPACTION_PROCESS_TOOL = 'context_compaction';
/** 压缩事件类型：这些条目的"身份"来自事件类型，不来自 tool 字段。 */
export const COMPACTION_EVENT_TYPES = new Set(['compaction', 'compaction_progress', 'compaction_notice']);

export const asObject = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};

const safeJsonParse = (text: string): unknown => {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
};

/** ThreadLog 条目封装：payload 里可能再嵌一层原始事件体，摊平成一份数据。 */
export const flattenItemPayload = (payload: unknown): Record<string, unknown> => {
  const outer = asObject(payload);
  const inner = asObject(outer.payload);
  const nestedData = { ...asObject(outer.data), ...asObject(inner.data) };
  if (Object.keys(inner).length === 0 && Object.keys(nestedData).length === 0) return outer;
  const merged: Record<string, unknown> = { ...outer, ...inner, ...nestedData };
  for (const key of ['tool', 'tool_name', 'tool_call_id', 'model_round', 'process_seq', 'status', 'kind', 'event_type']) {
    if (inner[key] !== undefined || outer[key] !== undefined) merged[key] = inner[key] ?? outer[key];
  }
  delete merged.payload;
  return merged;
};

export const pickText = (source: Record<string, unknown>, keys: string[]): string => {
  for (const key of keys) {
    const value = source[key];
    if (typeof value === 'string' && value.trim()) return value.trim();
  }
  return '';
};

export const stringifyDetail = (value: unknown): string => {
  try {
    const text = JSON.stringify(value ?? {});
    return text.length > 131072 ? `${text.slice(0, 131072)}…` : text;
  } catch {
    return '';
  }
};

/**
 * 自动压缩另外会写一条带显式标记的 system_message（摘要就是这条记录的正文）。
 * 普通系统提示词不是工作过程条目，只有带压缩标记的才进过程区。
 */
export const isCompactionSystemItem = (item: Record<string, unknown>): boolean => {
  if (String(item?.kind || '') !== 'system_message') return false;
  const data = flattenItemPayload(item?.payload);
  const meta = asObject(data.meta);
  const type = String(meta.type ?? data.type ?? '').trim().toLowerCase();
  const id = String(meta.compaction_id ?? meta.compactionId ?? data.compaction_id ?? data.compactionId ?? '').trim();
  return type === 'compaction_summary' || Boolean(id);
};

/** 该条目是否是压缩活动：独立 compaction 记录，或带压缩标记的 system_message。 */
export const isCompactionItem = (item: Record<string, unknown>, kind = String(item?.kind || '')): boolean =>
  COMPACTION_ITEM_KINDS.has(kind) || isCompactionSystemItem(item);

export type PersistedToolCallIdentity = { name: string; functionName: string; arguments?: unknown };

export const collectAssistantToolCalls = (
  items: Array<Record<string, unknown>>
): Map<string, PersistedToolCallIdentity> => {
  const identities = new Map<string, PersistedToolCallIdentity>();
  for (const item of items) {
    if (String(item?.kind || '') !== 'assistant_message') continue;
    const data = flattenItemPayload(item?.payload);
    const calls = data.tool_calls ?? data.toolCalls;
    if (!Array.isArray(calls)) continue;
    for (const rawCall of calls) {
      const call = asObject(rawCall);
      if (!call) continue;
      const functionPayload = Object.keys(asObject(call.function)).length > 0
        ? asObject(call.function)
        : call;
      const callId = pickText(call, ['id', 'tool_call_id', 'toolCallId', 'call_id', 'callId']);
      const name = pickText(functionPayload, ['name', 'tool', 'tool_name', 'toolName']) ||
        pickText(call, ['name', 'tool', 'tool_name', 'toolName']);
      if (!callId || !name) continue;
      identities.set(callId, {
        name,
        functionName: pickText(functionPayload, ['name', 'function_name', 'functionName']) || name,
        arguments: functionPayload.arguments ?? call.arguments
      });
    }
  }
  return identities;
};

/** 压缩条目的摘要正文：摘要优先，`content` 只是 `history` 之类的占位标记。 */
const resolveCompactionSummary = (
  data: Record<string, unknown>,
  fromSystemMarker: boolean
): string => {
  const summary = pickText(data, [
    'summary_text', 'summaryText', 'summary_model_output', 'summaryModelOutput'
  ]);
  if (summary) return summary;
  const content = typeof data.content === 'string' ? data.content.trim() : '';
  if (!content) return '';
  // 自动压缩把注入的摘要写在 system 记录正文里；独立 compaction 记录的
  // content 只是原因标记（例如 "history"），不能当摘要展示。
  if (fromSystemMarker) return content;
  return content.toLowerCase() === 'history' ? '' : content;
};

/**
 * 持久条目 → WorkflowItem：字段口径与运行时 reducer 的投影条目一致
 * （eventType / tool 身份 / call 引用 / detail），让 run 折叠与条目渲染
 * 直接复用 toolTimelineModel 的既有链路。
 */
export const mapWorkflowItem = (
  item: Record<string, unknown>,
  persistedToolCalls: ReadonlyMap<string, PersistedToolCallIdentity>
): WorkflowItem | null => {
  const kind = String(item?.kind || '');
  if (!kind || kind === 'assistant_message' || SKIP_KINDS.has(kind)) return null;
  const fromSystemMarker = kind === 'system_message';
  const compaction = isCompactionItem(item, kind);
  if (fromSystemMarker && !compaction) return null;
  const data = flattenItemPayload(item?.payload);
  const status = String(item?.status || data.status || 'completed');
  const eventType =
    fromSystemMarker
      ? 'compaction'
      : kind === 'tool_call'
      ? TERMINAL_TOOL_STATUS.has(status)
        ? 'tool_result'
        : 'tool_call'
      : String(data.event_type || data.eventType || kind);
  const mapped: WorkflowItem = {
    id: String(item?.item_id || ''),
    eventType,
    status,
    isTool: true,
    detail: stringifyDetail(data),
    updatedSeq: Number(item?.revision) || undefined
  };
  // 模型轮次：进程条目右侧的 R1/R2。持久条目把它写在 item 或 payload 上，
  // 旧记录没有该字段时折叠行不印轮次。
  const itemRound = Number(item?.model_round ?? data.model_round ?? data.modelRound);
  if (Number.isSafeInteger(itemRound) && itemRound > 0) mapped.model_round = itemRound;
  if (compaction) {
    // 压缩条目没有 tool 字段，身份只能由这里给：否则下面的"无身份条目"
    // 过滤会把整条压缩活动丢掉（桌面端同样按 kind 直接认成压缩条目）。
    mapped.toolName = COMPACTION_PROCESS_TOOL;
    mapped.tool = COMPACTION_PROCESS_TOOL;
    const summary = resolveCompactionSummary(data, fromSystemMarker);
    if (summary) {
      mapped.detail = stringifyDetail({ ...data, summary_text: summary });
    }
  }
  const callId = pickText(data, ['tool_call_id', 'toolCallId', 'call_id', 'callId']);
  const persisted = callId ? persistedToolCalls.get(callId) : undefined;
  const toolName = pickText(data, ['tool', 'name', 'tool_name', 'toolName']) || persisted?.name || '';
  if (toolName && !compaction) {
    mapped.toolName = toolName;
    mapped.tool = toolName;
  }
  const displayName = pickText(data, ['tool_display_name', 'toolDisplayName', 'display_name', 'displayName']);
  if (displayName) {
    mapped.toolDisplayName = displayName;
    mapped.tool_display_name = displayName;
  }
  const runtimeName = pickText(data, ['tool_runtime_name', 'toolRuntimeName', 'runtime_name', 'runtimeName']);
  if (runtimeName) {
    mapped.toolRuntimeName = runtimeName;
    mapped.tool_runtime_name = runtimeName;
  }
  const functionName = pickText(data, ['tool_function_name', 'toolFunctionName', 'function_name', 'functionName']) || persisted?.functionName || '';
  if (functionName) {
    mapped.toolFunctionName = functionName;
    mapped.tool_function_name = functionName;
  }
  if (callId) {
    mapped.toolCallId = callId;
    mapped.tool_call_id = callId;
  }
  const commandSessionId = pickText(data, ['command_session_id', 'commandSessionId']);
  if (commandSessionId) {
    mapped.commandSessionId = commandSessionId;
    mapped.command_session_id = commandSessionId;
  }
  // 补丁/编辑类条目需要原始调用参数（与 reducer 的 buildProjectedToolCallRawDetail 同口径）。
  const nestedFunction = asObject(data.function);
  const args = data.args ?? data.arguments ?? data.input ?? nestedFunction.arguments ?? persisted?.arguments;
  if (args !== undefined && (eventType === 'tool_call' || eventType === 'tool_result')) {
    const parsed = typeof args === 'string' ? safeJsonParse(args) : args;
    const rawCall = stringifyDetail({ tool: toolName || functionName || runtimeName, arguments: parsed });
    if (rawCall) {
      mapped.toolCallRawDetail = rawCall;
      mapped.tool_call_raw_detail = rawCall;
    }
  }
  return mapped;
};

/**
 * 一条过程条目是否已经拿到可展示的身份。工具行必须知道调用的是什么工具
 * （生命周期行先到、身份后到时不能先渲染成"未知工具"）；压缩行的身份来自
 * 事件类型，不要求 tool 字段。
 */
export const hasProcessIdentity = (mapped: WorkflowItem): boolean =>
  Boolean(String(mapped.toolName || mapped.tool || mapped.tool_name || '').trim()) ||
  COMPACTION_EVENT_TYPES.has(String(mapped.eventType || '').toLowerCase());

export type ProcessSlot =
  | { type: 'message'; item: Record<string, unknown> }
  | { type: 'tool'; item: Record<string, unknown> };

export type ProcessSlotSelection = {
  slots: ProcessSlot[];
  mappedItems: WorkflowItem[];
};

/**
 * 把一页持久条目分成"正文槽位"和"过程条目"。正文槽位只保留稳定身份的
 * 助手打印（稳定身份 `<turn_id>:text-<round>`），其余助手记录属于过程区或被去重逻辑收拢。
 */
export const collectProcessSlots = (items: Array<Record<string, unknown>>): ProcessSlotSelection => {
  const slots: ProcessSlot[] = [];
  const mappedItems: WorkflowItem[] = [];
  if (!Array.isArray(items) || items.length === 0) return { slots, mappedItems };
  const persistedToolCalls = collectAssistantToolCalls(items);
  for (const item of items) {
    const kind = String(item?.kind || '');
    if (SKIP_KINDS.has(kind)) continue;
    if (kind === 'system_message' && !isCompactionSystemItem(item)) continue;
    if (kind === 'assistant_message') {
      const payload = flattenItemPayload(item.payload);
      const execution = String(item.turn_id || payload.turn_id || '');
      const round = Number(item.model_round ?? payload.model_round ?? String(item.item_id).match(/:text-(\d+)$/)?.[1] ?? 0);
      if (execution && String(item.item_id) !== `${execution}:text-${round}`) continue;
      slots.push({ type: 'message', item });
      continue;
    }
    const mapped = mapWorkflowItem(item, persistedToolCalls);
    if (!mapped) continue;
    // A lifecycle row can precede the decorated tool result during replay.
    // Keep the process timeline quiet until its identity is known; otherwise
    // the renderer turns the incomplete row into a misleading “unknown tool”.
    if (!hasProcessIdentity(mapped)) continue;
    slots.push({ type: 'tool', item });
    mappedItems.push(mapped);
  }
  return { slots, mappedItems };
};
