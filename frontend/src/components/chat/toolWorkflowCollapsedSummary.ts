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

const compactPath = (value: string): string => {
  const normalized = value.replace(/\\/g, '/').replace(/\/+/g, '/').trim();
  if (!normalized) return '';
  const segments = normalized.split('/').filter(Boolean);
  if (segments.length <= 2) return normalized;
  return `.../${segments.slice(-2).join('/')}`;
};

// Collapsed rows intentionally inspect only a bounded prefix of raw payloads.
// Full JSON parsing and result formatting remain an explicit expand-time cost.
export const buildCollapsedToolWorkflowSummary = (
  entry: RawToolRun,
  toolLabel: string
): WorkflowToolSummary => {
  const items = [entry.callItem, entry.outputItem, entry.resultItem];
  let brief = '';
  if (isExecuteCommandTool(entry.toolName)) {
    brief = readEntryField(items, COMMAND_KEYS);
  } else if (isReadImageWorkflowTool(entry.toolName)) {
    brief = compactPath(readEntryField(items, PATH_KEYS));
  } else if (isWebFetchTool(entry.toolName)) {
    brief = readEntryField(items, URL_KEYS) || readEntryField(items, QUERY_KEYS);
  } else if (isQueryTool(entry.toolName)) {
    brief = readEntryField(items, QUERY_KEYS) || compactPath(readEntryField(items, PATH_KEYS));
  } else {
    brief = compactPath(readEntryField(items, PATH_KEYS)) ||
      readEntryField(items, QUERY_KEYS) ||
      readEntryField(items, ['action', 'operation', 'op']);
  }
  brief = compactText(brief);
  return {
    title: brief ? `${toolLabel} ${brief}` : toolLabel,
    brief
  };
};
