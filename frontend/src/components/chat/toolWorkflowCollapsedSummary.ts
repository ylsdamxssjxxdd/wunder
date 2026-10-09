// AI生成
import type { RawToolRun, WorkflowItem } from './toolWorkflowRunModel';

type WorkflowToolSummary = {
  title: string;
  brief: string;
};

const SCAN_LIMIT = 8_192;
const VALUE_LIMIT = 104;
const PATH_KEYS = ['path', 'file_path', 'file', 'filename', 'source_path', 'source'] as const;
const COMMAND_KEYS = ['content', 'command', 'cmd', 'input', 'script', 'raw'] as const;
const QUERY_KEYS = ['query', 'question', 'keyword', 'keywords', 'sql'] as const;
// `web_search` 现在以 `queries: string[]` 批处理调用（对齐 dsh web seam），
// 数组形态无法被按字符串取值的 readLightweightField 命中，单独读取。
const QUERY_BATCH_KEYS = ['queries'] as const;
const URL_KEYS = ['url', 'uri', 'source_url'] as const;

const normalizeToolName = (value: unknown): string => String(value || '').trim().toLowerCase();

export const isReadImageWorkflowTool = (toolName: unknown): boolean => {
  const normalized = normalizeToolName(toolName);
  return normalized === 'read_image' ||
    normalized === 'view_image' ||
    normalized === '\u8bfb\u56fe\u5de5\u5177' ||
    normalized === '\u8bfb\u56fe';
};

const isExecuteCommandTool = (toolName: unknown): boolean => {
  const normalized = normalizeToolName(toolName);
  return normalized === 'execute_command' || normalized.includes('\u6267\u884c\u547d\u4ee4');
};

const isQueryTool = (toolName: unknown): boolean => {
  const normalized = normalizeToolName(toolName);
  return normalized === 'search_content' ||
    normalized === 'web_search' ||
    normalized === 'web_fetch' ||
    normalized === 'db_query' ||
    normalized.startsWith('db_query_') ||
    normalized === 'kb_query' ||
    normalized.startsWith('kb_query_') ||
    normalized.includes('@db_query') ||
    normalized.includes('@kb_query');
};

const isWebFetchTool = (toolName: unknown): boolean => {
  const normalized = normalizeToolName(toolName);
  return normalized === 'web_fetch' || normalized === 'webfetch' || normalized.includes('web_fetch');
};

const compactText = (value: unknown, maxLength = VALUE_LIMIT): string => {
  const normalized = String(value || '').replace(/\s+/g, ' ').trim();
  if (!normalized) return '';
  return normalized.length > maxLength ? `${normalized.slice(0, maxLength)}...` : normalized;
};

const decodeJsonString = (value: string): string => {
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value
      .replace(/\\"/g, '"')
      .replace(/\\\\/g, '\\')
      .replace(/\\n/g, ' ')
      .replace(/\\r/g, ' ')
      .replace(/\\t/g, ' ');
  }
};

const readLightweightField = (source: string, keys: readonly string[]): string => {
  if (!source) return '';
  const sample = source.slice(0, SCAN_LIMIT);
  for (const key of keys) {
    const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const match = new RegExp(`"${escapedKey}"\\s*:\\s*"((?:\\\\.|[^"\\\\])*)"`, 'i').exec(sample);
    if (!match?.[1]) continue;
    const value = compactText(decodeJsonString(match[1]));
    if (value) return value;
  }
  return '';
};

const readEntryField = (items: Array<WorkflowItem | null>, keys: readonly string[]): string => {
  for (const item of items) {
    if (!item) continue;
    const record = item as WorkflowItem & Record<string, unknown>;
    for (const key of keys) {
      // `name` 同时是 WorkflowItem 的工具名字段：直接读记录字段会把工具名
      // 当成参数渲染；它只能来自调用参数 JSON 内部。
      if (key === 'name') continue;
      const direct = compactText(record[key]);
      if (direct) return direct;
    }
    const rawCall = typeof item.toolCallRawDetail === 'string'
      ? item.toolCallRawDetail
      : typeof item.tool_call_raw_detail === 'string'
        ? item.tool_call_raw_detail
        : '';
    const fromCall = readLightweightField(rawCall, keys);
    if (fromCall) return fromCall;
    const fromDetail = readLightweightField(typeof item.detail === 'string' ? item.detail : '', keys);
    if (fromDetail) return fromDetail;
  }
  return '';
};

/**
 * 结果/输出侧记录只允许贡献「调用的原始参数」（toolCallRawDetail）：
 * `detail` 与直取字段都是工具结果文本，进摘要就会把结果当成调用参数展示。
 * 持久化行会用结果记录顶替缺失的调用记录（保留 invocation JSON），这里兜住该形态。
 */
const readCallSideParam = (item: WorkflowItem | null, keys: readonly string[]): string => {
  if (!item) return '';
  const rawCall = typeof item.toolCallRawDetail === 'string'
    ? item.toolCallRawDetail
    : typeof item.tool_call_raw_detail === 'string'
      ? item.tool_call_raw_detail
      : '';
  return readLightweightField(rawCall, keys);
};

/**
 * 读取 `"key": ["a", "b"]` 形态的字符串数组参数，按 `, ` 连接成单行摘要。
 * dsh 的 search 卡片标题就是 `queries.join(', ')`，这里保持一致。
 */
const readLightweightStringArray = (source: string, keys: readonly string[]): string => {
  if (!source) return '';
  const sample = source.slice(0, SCAN_LIMIT);
  for (const key of keys) {
    const escapedKey = key.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const match = new RegExp(`"${escapedKey}"\\s*:\\s*\\[([^\\]]*)\\]`, 'i').exec(sample);
    if (!match?.[1]) continue;
    const values = match[1]
      .split(',')
      .map((part) => compactText(decodeJsonString(part.trim().replace(/^"|"$/g, ''))))
      .filter(Boolean);
    if (values.length) return values.join(', ');
  }
  return '';
};

const readCallSideBatchParam = (item: WorkflowItem | null, keys: readonly string[]): string => {
  if (!item) return '';
  const rawCall = typeof item.toolCallRawDetail === 'string'
    ? item.toolCallRawDetail
    : typeof item.tool_call_raw_detail === 'string'
      ? item.tool_call_raw_detail
      : '';
  return readLightweightStringArray(rawCall, keys);
};

const compactPath = (value: string): string => {
  const normalized = value.replace(/\\/g, '/').replace(/\/+/g, '/').trim();
  if (!normalized) return '';
  const segments = normalized.split('/').filter(Boolean);
  if (segments.length <= 2) return normalized;
  return `.../${segments.slice(-2).join('/')}`;
};

const basenameOfPath = (value: string): string => {
  const normalized = value.replace(/\\/g, '/').replace(/\/+/g, '/').replace(/\/+$/, '').trim();
  if (!normalized) return '';
  const segments = normalized.split('/').filter(Boolean);
  return segments.at(-1) || normalized;
};

// Collapsed rows intentionally inspect only a bounded prefix of raw payloads.
// Full JSON parsing and result formatting remain an explicit expand-time cost.
export const buildCollapsedToolWorkflowSummary = (
  entry: RawToolRun,
  toolLabel: string
): WorkflowToolSummary => {
  // 摘要只描述「这次调用做了什么」：调用记录全量读，结果/输出记录仅兜底
  // 其携带的调用原始参数，绝不吃结果正文。
  let brief = '';
  if (isExecuteCommandTool(entry.toolName)) {
    brief = readEntryField([entry.callItem], COMMAND_KEYS) || readCallSideParam(entry.resultItem, COMMAND_KEYS);
  } else if (isReadImageWorkflowTool(entry.toolName)) {
    brief = basenameOfPath(
      readEntryField([entry.callItem], PATH_KEYS) || readCallSideParam(entry.resultItem, PATH_KEYS)
    );
  } else if (isWebFetchTool(entry.toolName)) {
    brief = readEntryField([entry.callItem], URL_KEYS) ||
      readEntryField([entry.callItem], QUERY_KEYS) ||
      readCallSideParam(entry.resultItem, URL_KEYS) ||
      readCallSideParam(entry.resultItem, QUERY_KEYS);
  } else if (isQueryTool(entry.toolName)) {
    brief = readCallSideBatchParam(entry.callItem, QUERY_BATCH_KEYS) ||
      readCallSideBatchParam(entry.resultItem, QUERY_BATCH_KEYS) ||
      readEntryField([entry.callItem], QUERY_KEYS) ||
      compactPath(readEntryField([entry.callItem], PATH_KEYS)) ||
      readCallSideParam(entry.resultItem, QUERY_KEYS) ||
      compactPath(readCallSideParam(entry.resultItem, PATH_KEYS));
  } else {
    brief = compactPath(readEntryField([entry.callItem], PATH_KEYS)) ||
      readEntryField([entry.callItem], QUERY_KEYS) ||
      readEntryField([entry.callItem], ['action', 'operation', 'op']) ||
      compactPath(readCallSideParam(entry.resultItem, PATH_KEYS)) ||
      readCallSideParam(entry.resultItem, QUERY_KEYS);
  }
  brief = compactText(brief);
  return {
    title: brief ? `${toolLabel} ${brief}` : toolLabel,
    brief
  };
};
