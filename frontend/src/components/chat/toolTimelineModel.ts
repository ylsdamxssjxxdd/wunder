/**
 * B3 · 时间线条目化：把一条助手消息的工具运行时投影折成时间线条目。
 *
 * 目标形态（方案 §7.3 / §7.4 / §7.6）：
 * - 每次工具调用（或上下文压缩）是一个条目，带状态、工具显示名、目标标签；
 * - 补丁类工具体自带 diff 卡片数据（只做限量，不做二次解析）；
 * - 条目内容有界：超出上限的条目在渲染层截断并给出省略提示。
 *
 * 这里只做「有界的折叠」，不做整段历史重算：调用方（时间线段）只处理当前
 * 挂载的消息，`buildWorkflowToolRuns` 只吃有界窗口。
 */

import {
  buildWorkflowToolRuns,
  type RawToolRun,
  type WorkflowItem
} from './toolWorkflowRunModel';
import { buildCollapsedToolWorkflowSummary } from './toolWorkflowCollapsedSummary';
import {
  buildCompactionDisplay,
  resolveCompactionInstanceLabel,
  type CompactionDisplay
} from '@/utils/chatCompactionUi';

export type TimelineEntryStatus = 'loading' | 'completed' | 'failed' | 'cancelled';

export type TimelineTargetChip = {
  key: string;
  label: string;
  /** 工作区相对路径；可点击定位到左栏工作目录区。 */
  path: string;
};

export type TimelineToolEntry = {
  /** 条目类型：工具调用 / 上下文压缩。 */
  kind: 'tool' | 'compaction';
  key: string;
  status: TimelineEntryStatus;
  /** 工具显示名。 */
  toolLabel: string;
  toolIconClass: string;
  /** 一行摘要（执行中的实时摘要 / 完成后的结果摘要）。 */
  summary: string;
  /** 目标标签（文件名 / 路径），点击在左栏工作目录区定位。 */
  targets: TimelineTargetChip[];
  /** 失败时的错误摘要（可展开查看）。 */
  errorText: string;
  /** 结果正文 / 命令输出等长文本（渲染层限高折叠）。 */
  detailText: string;
  /** 是否存在可展开内容。 */
  expandable: boolean;
  /** 上下文压缩视图（压缩条目专用）。 */
  compaction: CompactionDisplay | null;
  /** 工具原始执行记录，供补丁卡片与子智能体面板复用。 */
  run: RawToolRun;
};

export type TimelineReasoningEntry = {
  kind: 'reasoning';
  key: string;
  streaming: boolean;
  /** 一行摘要（超长由渲染层省略）。 */
  summary: string;
  /** 完整思考内容（展开后限高滚动）。 */
  text: string;
};

export type TimelineEntry = TimelineToolEntry | TimelineReasoningEntry;

type UnknownObject = Record<string, unknown>;

const PATH_KEYS = [
  'path',
  'file',
  'filename',
  'file_path',
  'filePath',
  'target',
  'target_path',
  'targetPath',
  'source',
  'source_path',
  'sourcePath',
  'to_path',
  'toPath',
  'output_path',
  'outputPath',
  'input_path',
  'inputPath',
  'dir',
  'directory',
  'workdir',
  'cwd'
] as const;

const TEXT_KEYS = ['command', 'cmd', 'query', 'question', 'keyword', 'keywords', 'url', 'uri', 'skill', 'name'] as const;

const CONTEXT_CN = '\u4e0a\u4e0b\u6587';
const COMPACTION_CN = '\u538b\u7f29';
const COMPACTION_EVENT_TYPES = new Set(['compaction', 'compaction_progress', 'compaction_notice']);

const SUMMARY_LIMIT = 160;
const TARGET_LIMIT = 4;
const TARGET_LABEL_LIMIT = 48;

export const TIMELINE_ENTRY_RENDER_LIMIT = 40;
export const TIMELINE_ENTRY_PAGE_SIZE = 40;

export const normalizeTimelineStatus = (status: unknown): TimelineEntryStatus => {
  const value = String(status || '').trim().toLowerCase();
  if (value === 'cancelled' || value === 'canceled') return 'cancelled';
  if (
    value === 'failed' ||
    value === 'error' ||
    value === 'failure' ||
    value === 'failed_to_start' ||
    value === 'timeout' ||
    value === 'timed_out'
  ) {
    return 'failed';
  }
  if (value === 'running' || value === 'streaming' || value === 'started' || value === 'queued' ||
    value === 'loading' || value === 'pending') {
    return 'loading';
  }
  return 'completed';
};

const asObject = (value: unknown): UnknownObject | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as UnknownObject;
};

const pickString = (...candidates: unknown[]): string => {
  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim()) {
      return candidate.trim();
    }
  }
  return '';
};

const toOptionalInt = (...values: unknown[]): number | null => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value)) return Math.trunc(value);
    if (typeof value === 'string') {
      const parsed = Number.parseInt(value.trim(), 10);
      if (Number.isFinite(parsed)) return parsed;
    }
  }
  return null;
};

const truncateSingleLine = (text: unknown, maxLength = SUMMARY_LIMIT): string => {
  const normalized = String(text ?? '').replace(/\s+/g, ' ').trim();
  if (!normalized) return '';
  return normalized.length > maxLength ? `${normalized.slice(0, maxLength)}…` : normalized;
};

const parseDetailObject = (detail: unknown): UnknownObject | null => {
  if (typeof detail !== 'string') return null;
  const trimmed = detail.trim();
  if (!trimmed || (trimmed[0] !== '{' && trimmed[0] !== '[')) return null;
  try {
    return asObject(JSON.parse(trimmed));
  } catch {
    return null;
  }
};

const normalizePathLike = (value: unknown): string =>
  String(value || '')
    .trim()
    .replace(/\\/g, '/')
    .replace(/\/{2,}/g, '/');

const basenameOfPath = (value: string): string => {
  const segments = normalizePathLike(value).replace(/\/+$/, '').split('/').filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] : '';
};

const compactPathLabel = (value: string): string => {
  const normalized = normalizePathLike(value);
  if (!normalized) return '';
  const segments = normalized.split('/').filter(Boolean);
  if (segments.length <= 2) return normalized;
  return `…/${segments.slice(-2).join('/')}`;
};

/**
 * 从工具调用/结果的原始 detail 里取路径与关键文本。
 * 只扫描有界前缀，避免整段大 JSON 解析进入流式热路径。
 */
const readRawObjectFields = (detail: unknown): { paths: string[]; texts: string[] } => {
  const direct = asObject(detail);
  if (direct) {
    return readFieldsFromObject(direct);
  }
  if (typeof detail !== 'string' || !detail) return { paths: [], texts: [] };
  const parsed = parseDetailObject(detail.slice(0, 8192));
  if (parsed) return readFieldsFromObject(parsed);
  const sample = detail.slice(0, 4096);
  const paths: string[] = [];
  const texts: string[] = [];
  PATH_KEYS.forEach((key) => {
    const match = new RegExp(`"${key}"\\s*:\\s*"((?:\\\\.|[^"\\\\])*)"`, 'i').exec(sample);
    if (match?.[1]) paths.push(decodeJsonString(match[1]));
  });
  TEXT_KEYS.forEach((key) => {
    const match = new RegExp(`"${key}"\\s*:\\s*"((?:\\\\.|[^"\\\\])*)"`, 'i').exec(sample);
    if (match?.[1]) texts.push(decodeJsonString(match[1]));
  });
  return { paths, texts };
};

const readFieldsFromObject = (source: UnknownObject): { paths: string[]; texts: string[] } => {
  const paths: string[] = [];
  const texts: string[] = [];
  PATH_KEYS.forEach((key) => {
    const value = pickString(source[key]);
    if (value) paths.push(value);
  });
  TEXT_KEYS.forEach((key) => {
    const value = pickString(source[key]);
    if (value) texts.push(value);
  });
  const args = asObject(source.arguments) || asObject(source.args);
  if (args) {
    const nested = readFieldsFromObject(args);
    paths.push(...nested.paths);
    texts.push(...nested.texts);
  }
  return { paths, texts };
};

const decodeJsonString = (value: string): string => {
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value.replace(/\\"/g, '"').replace(/\\\\/g, '\\').replace(/\\n/g, ' ').trim();
  }
};

const isPatchToolName = (toolName: unknown): boolean => {
  const normalized = String(toolName || '').trim().toLowerCase();
  return normalized === 'apply_patch' || String(toolName || '').includes('\u5e94\u7528\u8865\u4e01');
};

const isCompactionToolName = (value: unknown): boolean => {
  const normalized = String(value || '').trim().toLowerCase();
  if (!normalized) return false;
  if (
    normalized === 'context_compaction' ||
    normalized === 'context_compact' ||
    normalized === 'compact_context' ||
    normalized === 'compaction' ||
    normalized === `${CONTEXT_CN}${COMPACTION_CN}`
  ) {
    return true;
  }
  if (normalized.includes('context') && (normalized.includes('compact') || normalized.includes('compaction'))) {
    return true;
  }
  return normalized.includes(CONTEXT_CN) && normalized.includes(COMPACTION_CN);
};

const isCompactionEventItem = (item: WorkflowItem | null | undefined): boolean =>
  Boolean(
    item &&
      (
        COMPACTION_EVENT_TYPES.has(String(item.eventType ?? item.event ?? item.event_type ?? '').trim().toLowerCase()) ||
        isCompactionToolName(item.toolName ?? item.tool ?? item.tool_name ?? item.name)
      )
  );

export const isCompactionRun = (run: RawToolRun): boolean =>
  isCompactionToolName(run.toolName) ||
  isCompactionEventItem(run.callItem) ||
  isCompactionEventItem(run.outputItem) ||
  isCompactionEventItem(run.resultItem);

const resolveRunStatus = (run: RawToolRun): TimelineEntryStatus => {
  const candidates = [run.resultItem?.status, run.outputItem?.status, run.callItem?.status];
  const detailObjects = [
    parseDetailObject(run.resultItem?.detail),
    parseDetailObject(run.callItem?.detail)
  ].filter(Boolean) as UnknownObject[];
  for (const detail of detailObjects) {
    const data = asObject(detail.data) || detail;
    const nested = asObject(data.result) || data;
    const timedOut = nested.timed_out === true || nested.timedOut === true;
    const errorText = pickString(nested.error, nested.error_message, nested.errorMessage);
    const exitCode = toOptionalInt(
      nested.exit_code,
      nested.exitCode,
      nested.returncode,
      nested.return_code,
      nested.returnCode
    );
    if (timedOut || errorText) return 'failed';
    if (exitCode !== null && exitCode !== 0) return 'failed';
  }
  for (const candidate of candidates) {
    if (!candidate) continue;
    return normalizeTimelineStatus(candidate);
  }
  return 'completed';
};

const resolveRunErrorText = (run: RawToolRun): string => {
  const details = [
    parseDetailObject(run.resultItem?.detail),
    parseDetailObject(run.outputItem?.detail),
    parseDetailObject(run.callItem?.detail)
  ].filter(Boolean) as UnknownObject[];
  for (const detail of details) {
    const data = asObject(detail.data) || detail;
    const nested = asObject(data.result) || data;
    const errorMeta = asObject(nested.error_meta) || asObject(data.error_meta);
    const summary = pickString(
      nested.failure_summary,
      nested.error_detail_head,
      nested.error,
      nested.error_message,
      nested.message,
      nested.stderr
    );
    const hint = pickString(nested.next_step_hint, nested.hint, errorMeta?.hint);
    const code = pickString(nested.error_code, nested.errorCode, errorMeta?.code);
    const composed = [summary && code ? `${summary} (${code})` : summary, hint].filter(Boolean).join('\n');
    if (composed) return composed;
  }
  return '';
};

const resolveRunDetailText = (run: RawToolRun): string => {
  const source = run.resultItem || run.outputItem;
  if (!source) return '';
  const detailObject = parseDetailObject(source.detail);
  if (!detailObject) {
    return typeof source.detail === 'string' ? source.detail : '';
  }
  const data = asObject(detailObject.data) || detailObject;
  const nested = asObject(data.result) || data;
  const text = pickString(
    nested.stdout,
    nested.output,
    typeof nested.content === 'string' ? nested.content : '',
    nested.text,
    nested.summary,
    nested.message,
    data.stdout,
    data.output,
    typeof data.content === 'string' ? data.content : ''
  );
  if (text) return text;
  const stderr = pickString(nested.stderr, data.stderr);
  return stderr;
};

const buildTargets = (run: RawToolRun): TimelineTargetChip[] => {
  const paths = new Set<string>();
  [run.resultItem, run.outputItem, run.callItem].forEach((item) => {
    if (!item) return;
    const fields = readRawObjectFields(item.detail);
    fields.paths.forEach((value) => {
      const normalized = normalizePathLike(value);
      // 只接受工作区相对路径形态，避免把服务端绝对路径渲染出来。
      if (!normalized || normalized.startsWith('/') || /^[a-zA-Z]:\//.test(normalized) || normalized.includes('..')) {
        return;
      }
      paths.add(normalized);
    });
  });
  return Array.from(paths)
    .slice(0, TARGET_LIMIT)
    .map((path) => {
      const label = compactPathLabel(path) || basenameOfPath(path);
      return {
        key: path,
        path,
        label: label.length > TARGET_LABEL_LIMIT ? `${label.slice(0, TARGET_LABEL_LIMIT)}…` : label
      };
    });
};

const buildSummaryFallback = (run: RawToolRun, toolLabel: string): string => {
  const fields = [run.callItem, run.resultItem].map((item) => readRawObjectFields(item?.detail));
  const text = fields.flatMap((item) => item.texts).find(Boolean) || '';
  if (text) return truncateSingleLine(text);
  return toolLabel;
};

const resolveCompactionEntry = (
  run: RawToolRun,
  t: (key: string, params?: Record<string, unknown>) => string
): TimelineToolEntry => {
  const detailObject =
    parseDetailObject(run.resultItem?.detail) ||
    parseDetailObject(run.outputItem?.detail) ||
    parseDetailObject(run.callItem?.detail);
  const status = resolveRunStatus(run);
  const display = buildCompactionDisplay(detailObject, status, t);
  const instanceLabel = resolveCompactionInstanceLabel(run.key, t);
  const toolLabel = t('chat.toolWorkflow.compaction.title');
  return {
    kind: 'compaction',
    key: run.key,
    status,
    toolLabel,
    toolIconClass: 'fa-compress',
    summary: truncateSingleLine([instanceLabel, display.summaryTitle || display.resultSummary].filter(Boolean).join(' · ')),
    targets: [],
    errorText: status === 'failed' ? display.summaryNote || display.resultSummary : '',
    detailText: display.resultBody || display.copyBody || '',
    expandable: Boolean(display.view) || Boolean(display.resultBody),
    compaction: display,
    run
  };
};

export const buildTimelineToolEntry = (
  run: RawToolRun,
  t: (key: string, params?: Record<string, unknown>) => string
): TimelineToolEntry => {
  if (isCompactionRun(run)) {
    return resolveCompactionEntry(run, t);
  }
  const toolName = pickString(run.toolName, run.toolRuntimeName, run.toolDisplayName);
  const toolLabel = pickString(run.toolDisplayName, toolName, t('chat.workflow.toolUnknown'));
  const status = resolveRunStatus(run);
  const collapsed = buildCollapsedToolWorkflowSummary(run, toolLabel);
  const targets = buildTargets(run);
  const errorText = status === 'failed' ? resolveRunErrorText(run) : '';
  const detailText = resolveRunDetailText(run);
  let summary = truncateSingleLine(collapsed.brief);
  if (!summary) {
    // 折叠摘要只扫有界前缀；没有命中时补一次轻量兜底，避免条目只有工具名。
    summary = status === 'loading'
      ? truncateSingleLine(t('chat.toolWorkflow.pendingToolDetail'))
      : buildSummaryFallback(run, '');
  }
  if (status === 'failed' && errorText) {
    summary = truncateSingleLine(errorText);
  }
  const expandable = Boolean(errorText || detailText || isPatchToolName(run.toolName) || run.resultItem);
  return {
    kind: 'tool',
    key: run.key,
    status,
    toolLabel,
    toolIconClass: resolveTimelineToolIcon(run.toolName),
    summary,
    targets,
    errorText,
    detailText,
    expandable,
    compaction: null,
    run
  };
};

const ICON_BY_TOOL: Array<{ match: string[]; icon: string }> = [
  { match: ['apply_patch', '\u5e94\u7528\u8865\u4e01'], icon: 'fa-file-pen' },
  { match: ['execute_command', '\u6267\u884c\u547d\u4ee4'], icon: 'fa-terminal' },
  { match: ['read_file', '\u8bfb\u53d6\u6587\u4ef6'], icon: 'fa-file-lines' },
  { match: ['read_image', 'view_image', '\u8bfb\u56fe'], icon: 'fa-image' },
  { match: ['write_file', '\u5199\u5165\u6587\u4ef6'], icon: 'fa-file-circle-plus' },
  { match: ['list_files', '\u5217\u51fa\u6587\u4ef6'], icon: 'fa-folder-tree' },
  { match: ['search_content', '\u641c\u7d22\u5185\u5bb9'], icon: 'fa-magnifying-glass' },
  { match: ['web_search', 'web_fetch', '\u7f51\u9875'], icon: 'fa-globe' },
  { match: ['skill', '\u6280\u80fd'], icon: 'fa-wand-magic-sparkles' }
];

const resolveTimelineToolIcon = (toolName: unknown): string => {
  const normalized = String(toolName || '').trim().toLowerCase();
  if (!normalized) return 'fa-toolbox';
  for (const candidate of ICON_BY_TOOL) {
    if (candidate.match.some((token) => normalized === token || normalized.includes(token))) {
      return candidate.icon;
    }
  }
  return 'fa-toolbox';
};

export type TimelineToolEntries = {
  runs: RawToolRun[];
  entries: TimelineToolEntry[];
  /** 有界窗口外的条目数量（用于「更早」提示）。 */
  omittedRuns: number;
};

/**
 * 折出有界的工具条目列表。
 * `sourceItems` 应当是已经按有界窗口切分的运行时投影。
 */
export const buildTimelineToolEntries = (
  sourceItems: WorkflowItem[],
  t: (key: string, params?: Record<string, unknown>) => string,
  limit = TIMELINE_ENTRY_RENDER_LIMIT
): TimelineToolEntries => {
  const runs = buildWorkflowToolRuns(Array.isArray(sourceItems) ? sourceItems : []);
  // 只看有界窗口：更早的记录不再参与折叠，避免长会话重复扫描。
  const visibleRuns = runs.length > limit ? runs.slice(runs.length - limit) : runs;
  return {
    runs: visibleRuns,
    entries: visibleRuns.map((run) => buildTimelineToolEntry(run, t)),
    omittedRuns: Math.max(runs.length - visibleRuns.length, 0)
  };
};
