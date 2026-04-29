export type WorkflowItem = {
  id?: string | number;
  itemId?: string | number;
  item_id?: string | number;
  title?: string;
  detail?: string;
  status?: string;
  isTool?: boolean;
  is_tool?: boolean;
  eventType?: string;
  event?: string;
  event_type?: string;
  toolName?: string;
  tool?: string;
  tool_name?: string;
  name?: string;
  toolDisplayName?: string;
  tool_display_name?: string;
  displayName?: string;
  display_name?: string;
  toolRuntimeName?: string;
  tool_runtime_name?: string;
  runtimeName?: string;
  runtime_name?: string;
  toolFunctionName?: string;
  tool_function_name?: string;
  functionName?: string;
  function_name?: string;
  toolCallId?: string | number;
  tool_call_id?: string | number;
  callId?: string | number;
  call_id?: string | number;
  commandSessionId?: string | number;
  command_session_id?: string | number;
};

export type RawToolRun = {
  key: string;
  toolName: string;
  toolDisplayName: string;
  toolRuntimeName: string;
  toolFunctionName: string;
  callItem: WorkflowItem | null;
  outputItem: WorkflowItem | null;
  resultItem: WorkflowItem | null;
};

type ToolEventKind = 'call' | 'output' | 'result';
export type WorkflowPendingPlaceholder = {
  kind: 'tool' | 'compaction';
  toolName: string;
  toolDisplayName: string;
  toolRuntimeName: string;
  toolFunctionName: string;
  eventType: string;
};

const COMMAND_SESSION_EVENT_TYPES = new Set([
  'command_session_start',
  'command_session_status',
  'command_session_exit',
  'command_session_summary',
  'command_session_delta'
]);

const normalizeWorkflowRef = (value: unknown): string => String(value || '').trim();
const normalizeWorkflowText = (value: unknown): string => String(value || '').trim();

const resolveWorkflowItemId = (item: WorkflowItem): string =>
  normalizeWorkflowRef(item.id ?? item.itemId ?? item.item_id);

const resolveWorkflowToolCallRef = (item: WorkflowItem): string =>
  normalizeWorkflowRef(item.toolCallId ?? item.tool_call_id ?? item.callId ?? item.call_id);

const resolveWorkflowCommandSessionRef = (item: WorkflowItem): string =>
  normalizeWorkflowRef(item.commandSessionId ?? item.command_session_id);

const resolveWorkflowLinkRef = (item: WorkflowItem): string =>
  resolveWorkflowToolCallRef(item) || resolveWorkflowCommandSessionRef(item);

const resolveWorkflowEventType = (item: WorkflowItem): string =>
  normalizeWorkflowText(item.eventType ?? item.event ?? item.event_type).toLowerCase();

export const resolveWorkflowToolDisplayName = (item: WorkflowItem): string =>
  normalizeWorkflowText(
    item.toolDisplayName
      ?? item.tool_display_name
      ?? item.displayName
      ?? item.display_name
  );

export const resolveWorkflowToolRuntimeName = (item: WorkflowItem): string =>
  normalizeWorkflowText(
    item.toolRuntimeName
      ?? item.tool_runtime_name
      ?? item.runtimeName
      ?? item.runtime_name
  );

export const resolveWorkflowToolFunctionName = (item: WorkflowItem): string =>
  normalizeWorkflowText(
    item.toolFunctionName
      ?? item.tool_function_name
      ?? item.functionName
      ?? item.function_name
  );

export const resolveWorkflowToolName = (item: WorkflowItem): string => {
  const direct = normalizeWorkflowText(item.toolName ?? item.tool ?? item.tool_name ?? item.name);
  if (direct) return direct;
  const runtimeName = resolveWorkflowToolRuntimeName(item);
  if (runtimeName) return runtimeName;
  const functionName = resolveWorkflowToolFunctionName(item);
  if (functionName) return functionName;
  const rawTitle = normalizeWorkflowText(item.title);
  return rawTitle
    .replace(/^调用工具[:：]\s*/i, '')
    .replace(/^工具结果[:：]\s*/i, '')
    .replace(/^工具输出[:：]\s*/i, '')
    .replace(/^Tool\s+call:\s*/i, '')
    .replace(/^Tool\s+result:\s*/i, '')
    .replace(/^Tool\s+output:\s*/i, '')
    .trim();
};

export const resolveWorkflowToolEventKind = (item: WorkflowItem): ToolEventKind | null => {
  const eventType = resolveWorkflowEventType(item);
  if (eventType === 'tool_call') return 'call';
  if (eventType === 'tool_output_delta' || eventType === 'compaction_progress') return 'output';
  if (eventType === 'tool_result' || eventType === 'compaction') return 'result';

  const title = normalizeWorkflowText(item.title);
  if (/^调用工具[:：]/i.test(title) || /^Tool\s+call:/i.test(title)) return 'call';
  if (/^工具输出[:：]/i.test(title) || /^Tool\s+output:/i.test(title) || title === '工具输出') return 'output';
  if (/^工具结果[:：]/i.test(title) || /^Tool\s+result:/i.test(title)) return 'result';

  return item.isTool || item.is_tool ? 'result' : null;
};

export const resolveWorkflowPendingPlaceholder = (
  items: WorkflowItem[]
): WorkflowPendingPlaceholder | null => {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    const eventType = resolveWorkflowEventType(item);
    const toolName = resolveWorkflowToolName(item);
    const toolDisplayName = resolveWorkflowToolDisplayName(item);
    const toolRuntimeName = resolveWorkflowToolRuntimeName(item) || toolName;
    const toolFunctionName = resolveWorkflowToolFunctionName(item);
    const kindFromToolEvent = resolveWorkflowToolEventKind(item);
    if (eventType === 'compaction' || eventType === 'compaction_progress') {
      return {
        kind: 'compaction',
        toolName: toolName || '上下文压缩',
        toolDisplayName,
        toolRuntimeName,
        toolFunctionName,
        eventType
      };
    }
    if (kindFromToolEvent) {
      return {
        kind: toolName.trim() === '上下文压缩' ? 'compaction' : 'tool',
        toolName,
        toolDisplayName,
        toolRuntimeName,
        toolFunctionName,
        eventType
      };
    }
    if (COMMAND_SESSION_EVENT_TYPES.has(eventType)) {
      return {
        kind: 'tool',
        toolName,
        toolDisplayName,
        toolRuntimeName,
        toolFunctionName,
        eventType
      };
    }
    if (eventType.startsWith('tool_') || eventType.startsWith('subagent_') || eventType.startsWith('team_')) {
      return {
        kind: toolName.trim() === '上下文压缩' ? 'compaction' : 'tool',
        toolName,
        toolDisplayName,
        toolRuntimeName,
        toolFunctionName,
        eventType
      };
    }
    if (item.isTool || item.is_tool) {
      return {
        kind: toolName.trim() === '上下文压缩' ? 'compaction' : 'tool',
        toolName,
        toolDisplayName,
        toolRuntimeName,
        toolFunctionName,
        eventType
      };
    }
  }
  return null;
};

const dedupeAdjacentToolItems = (items: WorkflowItem[]): WorkflowItem[] => {
  const output: WorkflowItem[] = [];
  let lastKey = '';
  items.forEach((item) => {
    const kind = resolveWorkflowToolEventKind(item);
    if (!kind) {
      output.push(item);
      lastKey = '';
      return;
    }
    const key = [
      kind,
      resolveWorkflowToolName(item).trim().toLowerCase(),
      resolveWorkflowLinkRef(item),
      normalizeWorkflowText(item.status).toLowerCase(),
      normalizeWorkflowText(item.title),
      normalizeWorkflowText(item.detail)
    ].join('::');
    if (key && key === lastKey) {
      return;
    }
    output.push(item);
    lastKey = key;
  });
  return output;
};

const findLastPendingIndex = (rows: RawToolRun[]): number => {
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    if (!rows[index].resultItem) return index;
  }
  return -1;
};

export const buildWorkflowToolRuns = (items: WorkflowItem[]): RawToolRun[] => {
  const rows: RawToolRun[] = [];
  const pendingByTool = new Map<string, number[]>();
  const rowIndexByCallId = new Map<string, number>();
  const normalizedItems = dedupeAdjacentToolItems(items);

  const enqueuePending = (toolKey: string, index: number) => {
    if (!pendingByTool.has(toolKey)) pendingByTool.set(toolKey, []);
    const queue = pendingByTool.get(toolKey);
    if (!queue?.includes(index)) {
      queue?.push(index);
    }
  };

  const removePendingIndex = (toolKey: string, index: number) => {
    const queue = pendingByTool.get(toolKey);
    if (!queue?.length) return;
    const nextQueue = queue.filter((item) => item !== index);
    if (nextQueue.length > 0) {
      pendingByTool.set(toolKey, nextQueue);
    } else {
      pendingByTool.delete(toolKey);
    }
  };

  const pickPendingForOutput = (toolKey: string): number => {
    const queue = pendingByTool.get(toolKey);
    if (queue && queue.length > 0) return queue[queue.length - 1];
    return findLastPendingIndex(rows);
  };

  const pickPendingForResult = (toolKey: string): number => {
    const queue = pendingByTool.get(toolKey);
    if (queue && queue.length > 0) {
      const index = queue.shift();
      return typeof index === 'number' ? index : findLastPendingIndex(rows);
    }
    return findLastPendingIndex(rows);
  };

  const ensureRowForCallRef = (callRef: string, toolName: string, fallbackKey: string): number => {
    const normalizedRef = normalizeWorkflowRef(callRef);
    if (normalizedRef) {
      const existing = rowIndexByCallId.get(normalizedRef);
      if (typeof existing === 'number') return existing;
    }
    rows.push({
      key: normalizedRef || fallbackKey,
      toolName,
      toolDisplayName: '',
      toolRuntimeName: toolName,
      toolFunctionName: '',
      callItem: null,
      outputItem: null,
      resultItem: null
    });
    const rowIndex = rows.length - 1;
    if (normalizedRef) {
      rowIndexByCallId.set(normalizedRef, rowIndex);
    }
    return rowIndex;
  };

  // Keep output/result pairing stable even when the stream arrives before or after the call event.
  normalizedItems.forEach((item, index) => {
    const kind = resolveWorkflowToolEventKind(item);
    if (!kind) return;

    const toolName = resolveWorkflowToolName(item);
    const toolDisplayName = resolveWorkflowToolDisplayName(item);
    const toolRuntimeName = resolveWorkflowToolRuntimeName(item) || toolName;
    const toolFunctionName = resolveWorkflowToolFunctionName(item);
    const toolKey = toolName.trim().toLowerCase() || '__unknown__';
    const itemId = resolveWorkflowItemId(item) || `tool-entry-${index}`;
    const toolCallId = resolveWorkflowLinkRef(item);

    if (kind === 'call') {
      const existingIndex =
        (toolCallId ? rowIndexByCallId.get(toolCallId) : undefined) ?? rowIndexByCallId.get(itemId);
      if (typeof existingIndex === 'number') {
        rows[existingIndex].callItem = item;
        if (!rows[existingIndex].toolName && toolName) rows[existingIndex].toolName = toolName;
        if (!rows[existingIndex].toolDisplayName && toolDisplayName) rows[existingIndex].toolDisplayName = toolDisplayName;
        if (!rows[existingIndex].toolRuntimeName && toolRuntimeName) rows[existingIndex].toolRuntimeName = toolRuntimeName;
        if (!rows[existingIndex].toolFunctionName && toolFunctionName) rows[existingIndex].toolFunctionName = toolFunctionName;
        rowIndexByCallId.set(itemId, existingIndex);
        if (toolCallId) {
          rowIndexByCallId.set(toolCallId, existingIndex);
        }
        if (!rows[existingIndex].resultItem) {
          enqueuePending(toolKey, existingIndex);
        }
      } else {
        rows.push({
          key: itemId,
          toolName,
          toolDisplayName,
          toolRuntimeName,
          toolFunctionName,
          callItem: item,
          outputItem: null,
          resultItem: null
        });
        const rowIndex = rows.length - 1;
        rowIndexByCallId.set(itemId, rowIndex);
        if (toolCallId) {
          rowIndexByCallId.set(toolCallId, rowIndex);
        }
        enqueuePending(toolKey, rowIndex);
      }
      return;
    }

    if (kind === 'output') {
      let targetIndex =
        (toolCallId ? rowIndexByCallId.get(toolCallId) : undefined) ?? pickPendingForOutput(toolKey);
      if (targetIndex < 0 && toolCallId) {
        targetIndex = ensureRowForCallRef(toolCallId, toolName, itemId);
      }
      if (targetIndex >= 0) {
        rows[targetIndex].outputItem = item;
        if (!rows[targetIndex].toolName && toolName) rows[targetIndex].toolName = toolName;
        if (!rows[targetIndex].toolDisplayName && toolDisplayName) rows[targetIndex].toolDisplayName = toolDisplayName;
        if (!rows[targetIndex].toolRuntimeName && toolRuntimeName) rows[targetIndex].toolRuntimeName = toolRuntimeName;
        if (!rows[targetIndex].toolFunctionName && toolFunctionName) rows[targetIndex].toolFunctionName = toolFunctionName;
      } else {
        rows.push({
          key: itemId,
          toolName,
          toolDisplayName,
          toolRuntimeName,
          toolFunctionName,
          callItem: null,
          outputItem: item,
          resultItem: null
        });
      }
      return;
    }

    let targetIndex =
      (toolCallId ? rowIndexByCallId.get(toolCallId) : undefined) ?? pickPendingForResult(toolKey);
    if (typeof targetIndex === 'number' && targetIndex >= 0 && toolCallId) {
      removePendingIndex(toolKey, targetIndex);
    }
    if (targetIndex < 0 && toolCallId) {
      targetIndex = ensureRowForCallRef(toolCallId, toolName, itemId);
    }
    if (targetIndex >= 0) {
      rows[targetIndex].resultItem = item;
      if (!rows[targetIndex].toolName && toolName) rows[targetIndex].toolName = toolName;
      if (!rows[targetIndex].toolDisplayName && toolDisplayName) rows[targetIndex].toolDisplayName = toolDisplayName;
      if (!rows[targetIndex].toolRuntimeName && toolRuntimeName) rows[targetIndex].toolRuntimeName = toolRuntimeName;
      if (!rows[targetIndex].toolFunctionName && toolFunctionName) rows[targetIndex].toolFunctionName = toolFunctionName;
      if (toolCallId) {
        rowIndexByCallId.set(toolCallId, targetIndex);
      }
    } else {
      rows.push({
        key: itemId,
        toolName,
        toolDisplayName,
        toolRuntimeName,
        toolFunctionName,
        callItem: null,
        outputItem: null,
        resultItem: item
      });
    }
  });

  return rows;
};
