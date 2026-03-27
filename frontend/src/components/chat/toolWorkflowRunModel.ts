export type WorkflowItem = {
  id?: string | number;
  title?: string;
  detail?: string;
  status?: string;
  isTool?: boolean;
  eventType?: string;
  toolName?: string;
  toolCallId?: string | number;
  commandSessionId?: string | number;
};

export type RawToolRun = {
  key: string;
  toolName: string;
  callItem: WorkflowItem | null;
  outputItem: WorkflowItem | null;
  resultItem: WorkflowItem | null;
};

type ToolEventKind = 'call' | 'output' | 'result';

const normalizeWorkflowRef = (value: unknown): string => String(value || '').trim();

export const resolveWorkflowToolName = (item: WorkflowItem): string => {
  const direct = String(item.toolName || '').trim();
  if (direct) return direct;
  const rawTitle = String(item.title || '').trim();
  return rawTitle
    .replace(/^调用工具[:：]?\s*/i, '')
    .replace(/^工具结果[:：]?\s*/i, '')
    .replace(/^工具输出[:：]?\s*/i, '')
    .replace(/^Tool\s+call:\s*/i, '')
    .replace(/^Tool\s+result:\s*/i, '')
    .replace(/^Tool\s+output:\s*/i, '')
    .trim();
};

export const resolveWorkflowToolEventKind = (item: WorkflowItem): ToolEventKind | null => {
  const eventType = String(item.eventType || '').trim().toLowerCase();
  if (eventType === 'tool_call') return 'call';
  if (eventType === 'tool_output_delta' || eventType === 'compaction_progress') return 'output';
  if (eventType === 'tool_result' || eventType === 'compaction') return 'result';

  const title = String(item.title || '').trim();
  if (/^调用工具[:：]?/i.test(title) || /^Tool\s+call:/i.test(title)) return 'call';
  if (/^工具输出[:：]?/i.test(title) || /^Tool\s+output:/i.test(title) || title === '工具输出') return 'output';
  if (/^工具结果[:：]?/i.test(title) || /^Tool\s+result:/i.test(title)) return 'result';

  return item.isTool ? 'result' : null;
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
      normalizeWorkflowRef(item.toolCallId),
      String(item.status || '').trim().toLowerCase(),
      String(item.title || '').trim(),
      String(item.detail || '').trim()
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
    const toolKey = toolName.trim().toLowerCase() || '__unknown__';
    const itemId = normalizeWorkflowRef(item.id) || `tool-entry-${index}`;
    const toolCallId = normalizeWorkflowRef(item.toolCallId);

    if (kind === 'call') {
      const existingIndex =
        (toolCallId ? rowIndexByCallId.get(toolCallId) : undefined) ?? rowIndexByCallId.get(itemId);
      if (typeof existingIndex === 'number') {
        rows[existingIndex].callItem = item;
        if (!rows[existingIndex].toolName && toolName) rows[existingIndex].toolName = toolName;
        rowIndexByCallId.set(itemId, existingIndex);
        if (toolCallId) {
          rowIndexByCallId.set(toolCallId, existingIndex);
        }
        if (!rows[existingIndex].resultItem) {
          enqueuePending(toolKey, existingIndex);
        }
      } else {
        rows.push({ key: itemId, toolName, callItem: item, outputItem: null, resultItem: null });
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
      } else {
        rows.push({
          key: itemId,
          toolName,
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
      if (toolCallId) {
        rowIndexByCallId.set(toolCallId, targetIndex);
      }
    } else {
      rows.push({
        key: itemId,
        toolName,
        callItem: null,
        outputItem: null,
        resultItem: item
      });
    }
  });

  return rows;
};
