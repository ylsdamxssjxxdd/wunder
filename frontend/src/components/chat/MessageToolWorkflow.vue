<template>
  <div v-if="shouldRender" class="message-tool-workflow-shell">
    <details
      ref="workflowRef"
      class="message-tool-workflow"
      @toggle="handleWorkflowToggle"
    >
      <summary>
        <span class="tool-workflow-title">{{ t('chat.toolWorkflow.title') }}</span>
        <span v-if="latestEntry" class="tool-workflow-latest" :title="latestEntry.summaryTitle">
          {{ latestEntry.summaryTitle }}
        </span>
        <span v-else class="tool-workflow-spacer" />
      </summary>

      <div ref="workflowListRef" class="tool-workflow-list" @scroll="handleWorkflowScroll">
        <div v-if="entries.length === 0" class="tool-workflow-empty">{{ t('chat.toolWorkflow.empty') }}</div>

        <details
          v-for="entry in entries"
          :key="entry.key"
          class="tool-workflow-entry"
          :open="expandedKeys.has(entry.key)"
          @toggle="handleEntryToggle(entry.key, $event)"
        >
          <summary class="tool-workflow-entry-summary">
            <span :class="['tool-workflow-entry-lamp', `is-${entry.status}`]" aria-hidden="true"></span>
            <span class="tool-workflow-entry-title">{{ entry.summaryTitle }}</span>
            <span v-if="entry.durationLabel" class="tool-workflow-entry-duration">{{ entry.durationLabel }}</span>
            <span :class="['tool-workflow-entry-status', `is-${entry.status}`]">{{ entry.statusLabel }}</span>
          </summary>

          <div class="tool-workflow-entry-body">
            <MessageToolWorkflowSection
              v-for="section in entry.sections"
              :key="section.key"
              :section="section"
              :bind-stream-body-ref="(stream, el) => bindStreamBodyRef(entry.key, stream, el)"
              :on-stream-body-scroll="(stream, event) => handleStreamBodyScroll(entry.key, stream, event)"
            />
          </div>
        </details>
      </div>
    </details>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch, type ComponentPublicInstance } from 'vue';

import { useI18n } from '@/i18n';
import {
  useCommandSessionStore,
  type CommandSessionRuntimeEntry
} from '@/stores/commandSessions';
import {
  buildCommandCardView,
  buildPatchCallView,
  buildPatchResultView
} from './toolWorkflowActionViews';
import { buildToolResultPreview } from './toolWorkflowPreview';
import {
  buildWorkflowToolRuns,
  type RawToolRun as RawEntry,
  type WorkflowItem
} from './toolWorkflowRunModel';
import {
  buildStructuredToolResultView
} from './toolWorkflowStructuredView';
import { chatPerf } from '@/utils/chatPerf';
import {
  buildCompactionDisplay,
  type CompactionDisplay,
  type CompactionView
} from '@/utils/chatCompactionUi';
import MessageToolWorkflowSection from './MessageToolWorkflowSection.vue';
import type {
  ToolWorkflowCommandView as CommandView,
  ToolWorkflowDetailSection,
  ToolWorkflowPatchFileView as PatchFileView,
  ToolWorkflowPatchLine as PatchLine
} from './toolWorkflowTypes';

type PatchEntry = {
  key: string;
  kind: 'add' | 'delete' | 'update' | 'move' | 'other';
  sign: string;
  text: string;
};

type PatchDiffLine = {
  key: string;
  kind: 'add' | 'delete' | 'meta' | 'omit';
  text: string;
};

type PatchDiffBlock = {
  key: string;
  title: string;
  pathHint: string;
  lines: PatchDiffLine[];
};

type ToolEntryView = {
  key: string;
  summaryTitle: string;
  summaryNote: string;
  summaryNoteTone: '' | 'info' | 'success' | 'warning';
  isCompaction: boolean;
  compactionView: CompactionView | null;
  status: string;
  statusLabel: string;
  durationLabel: string;
  sections: ToolWorkflowDetailSection[];
};

type CommandRecord = {
  command: string;
  stdout: string;
  stderr: string;
  returncode: number | null;
};

type TerminalAutoStickMode = 'always' | 'smart' | 'off';
type CommandStreamName = 'stdout' | 'stderr';

type Props = {
  items?: WorkflowItem[];
  loading?: boolean;
  visible?: boolean;
  terminalAutoStick?: TerminalAutoStickMode;
};

type UnknownObject = Record<string, unknown>;

type RawPatchPreview = {
  action: PatchEntry['kind'];
  path: string;
  toPath: string;
  lines: string[];
  omitted: number;
};

const FILE_HINT_LIMIT = 5;
const FILE_HINT_SUMMARY_LIMIT = 2;
const PATCH_RESULT_FILE_LIMIT = 10;
const PATCH_PREVIEW_FILE_LIMIT = 4;
const PATCH_PREVIEW_LINE_LIMIT = 12;
const PATCH_PREVIEW_LINE_MAX_CHARS = 140;
const DETAIL_PARSE_CACHE_LIMIT = 120;
const PREVIEW_CACHE_LIMIT = 120;

const PATH_HINT_KEYS = [
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
  'destination',
  'dest',
  'workdir',
  'cwd',
  'dir',
  'directory',
  'output_path',
  'outputPath',
  'input_path',
  'inputPath'
];

const props = withDefaults(defineProps<Props>(), {
  items: () => [],
  loading: false,
  visible: false,
  terminalAutoStick: 'smart'
});
const emit = defineEmits<{
  (event: 'layout-change'): void;
}>();

const { t } = useI18n();
const commandSessionStore = useCommandSessionStore();
const expandedKeys = ref<Set<string>>(new Set());
const streamBodyRefMap = new Map<string, HTMLPreElement>();
const streamFollowState = new Map<string, boolean>();
const workflowRef = ref<HTMLDetailsElement | null>(null);
const workflowListRef = ref<HTMLElement | null>(null);
const workflowFollow = ref(true);
const detailParseCache = new Map<string, UnknownObject | false>();
const previewCache = new Map<string, string>();
let workflowLayoutFrame: number | null = null;

const streamKey = (entryKey: string, stream: CommandStreamName): string => `${entryKey}::${stream}`;

const isNearBottom = (element: HTMLElement, threshold = 20): boolean =>
  element.scrollTop + element.clientHeight >= element.scrollHeight - threshold;

const terminalAutoStickMode = computed<TerminalAutoStickMode>(() =>
  normalizeTerminalAutoStickMode(props.terminalAutoStick)
);

const shouldAutoStickStream = (key: string): boolean => {
  const mode = terminalAutoStickMode.value;
  if (mode === 'off') return false;
  if (mode === 'always') return true;
  return streamFollowState.get(key) ?? true;
};

const scrollStreamToBottom = (key: string) => {
  const element = streamBodyRefMap.get(key);
  if (!element) return;
  element.scrollTop = element.scrollHeight;
};

const shouldAutoScrollWorkflow = (): boolean => {
  if (!props.visible) return false;
  if (workflowRef.value && !workflowRef.value.open) return false;
  return workflowFollow.value;
};

const scrollWorkflowToBottom = () => {
  const element = workflowListRef.value;
  if (!element) return;
  element.scrollTop = element.scrollHeight;
};

const scheduleWorkflowLayoutChange = () => {
  if (typeof window === 'undefined') {
    emit('layout-change');
    return;
  }
  if (workflowLayoutFrame !== null) return;
  workflowLayoutFrame = window.requestAnimationFrame(() => {
    workflowLayoutFrame = null;
    emit('layout-change');
  });
};

const bindStreamBodyRef = (
  entryKey: string,
  stream: CommandStreamName,
  element: Element | ComponentPublicInstance | null
) => {
  const key = streamKey(entryKey, stream);
  if (!(element instanceof HTMLPreElement)) {
    streamBodyRefMap.delete(key);
    streamFollowState.delete(key);
    return;
  }
  streamBodyRefMap.set(key, element);
  if (!streamFollowState.has(key)) {
    streamFollowState.set(key, true);
  }
  if (shouldAutoStickStream(key)) {
    requestAnimationFrame(() => scrollStreamToBottom(key));
  }
};

const handleStreamBodyScroll = (entryKey: string, stream: CommandStreamName, event: Event) => {
  const target = event.target;
  if (!(target instanceof HTMLElement)) return;
  streamFollowState.set(streamKey(entryKey, stream), isNearBottom(target));
};

const handleWorkflowScroll = (event: Event) => {
  const target = event.target;
  if (!(target instanceof HTMLElement)) return;
  workflowFollow.value = isNearBottom(target);
};

const handleWorkflowToggle = (event: Event) => {
  const target = event.target;
  if (!(target instanceof HTMLDetailsElement)) return;
  if (target.open) {
    workflowFollow.value = true;
    void nextTick(() => {
      if (shouldAutoScrollWorkflow()) {
        scrollWorkflowToBottom();
      }
    });
  }
  scheduleWorkflowLayoutChange();
};

const syncStreamAutoStick = () => {
  streamBodyRefMap.forEach((_element, key) => {
    if (shouldAutoStickStream(key)) {
      scrollStreamToBottom(key);
    }
  });
};

const pruneStreamTracking = (validEntryKeys: Set<string>) => {
  Array.from(streamBodyRefMap.keys()).forEach((key) => {
    const entryKey = key.split('::', 1)[0];
    if (!validEntryKeys.has(entryKey)) {
      streamBodyRefMap.delete(key);
      streamFollowState.delete(key);
    }
  });
};

const asObject = (value: unknown): UnknownObject | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as UnknownObject;
};

const normalizeStatus = (status: unknown): string => {
  const value = String(status || '').trim().toLowerCase();
  if (value === 'loading' || value === 'pending' || value === 'failed' || value === 'completed') {
    return value;
  }
  return 'completed';
};

const statusLabel = (status: string): string => {
  if (status === 'pending') return t('chat.toolWorkflow.statusWaiting');
  if (status === 'loading') return t('chat.toolWorkflow.statusRunning');
  if (status === 'failed') return t('chat.toolWorkflow.statusFailed');
  return t('chat.toolWorkflow.statusSuccess');
};

const getCachedPreview = (key: string): string | null => {
  if (!key) return null;
  const cached = previewCache.get(key);
  if (!cached) return null;
  previewCache.delete(key);
  previewCache.set(key, cached);
  return cached;
};

const setCachedPreview = (key: string, value: string) => {
  if (!key) return;
  previewCache.set(key, value);
  if (previewCache.size > PREVIEW_CACHE_LIMIT) {
    const firstKey = previewCache.keys().next().value as string | undefined;
    if (firstKey) {
      previewCache.delete(firstKey);
    }
  }
};

const getCachedDetailObject = (detail: string): UnknownObject | false | undefined => {
  if (!detail) return undefined;
  if (!detailParseCache.has(detail)) return undefined;
  const cached = detailParseCache.get(detail);
  if (cached === undefined) return undefined;
  detailParseCache.delete(detail);
  detailParseCache.set(detail, cached);
  return cached;
};

const setCachedDetailObject = (detail: string, parsed: UnknownObject | false) => {
  if (!detail) return;
  detailParseCache.set(detail, parsed);
  if (detailParseCache.size > DETAIL_PARSE_CACHE_LIMIT) {
    const firstKey = detailParseCache.keys().next().value as string | undefined;
    if (firstKey) {
      detailParseCache.delete(firstKey);
    }
  }
};

const parseDetailObject = (detail: unknown): UnknownObject | null => {
  if (typeof detail !== 'string') return null;
  const trimmed = detail.trim();
  if (!trimmed || (trimmed[0] !== '{' && trimmed[0] !== '[')) return null;
  const cached = getCachedDetailObject(trimmed);
  if (cached !== undefined) {
    return cached === false ? null : cached;
  }
  try {
    const parsed = JSON.parse(trimmed);
    const normalized = asObject(parsed);
    setCachedDetailObject(trimmed, normalized ?? false);
    return normalized;
  } catch {
    setCachedDetailObject(trimmed, false);
    return null;
  }
};

const pickString = (...candidates: unknown[]): string => {
  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim()) {
      return candidate.trim();
    }
  }
  return '';
};

const toInt = (...values: unknown[]): number => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value) && value >= 0) {
      return Math.floor(value);
    }
    if (typeof value === 'string') {
      const parsed = Number.parseInt(value.trim(), 10);
      if (Number.isFinite(parsed) && parsed >= 0) return parsed;
    }
  }
  return 0;
};

const toOptionalInt = (...values: unknown[]): number | null => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value) && value >= 0) {
      return Math.floor(value);
    }
    if (typeof value === 'string') {
      const parsed = Number.parseInt(value.trim(), 10);
      if (Number.isFinite(parsed) && parsed >= 0) return parsed;
    }
  }
  return null;
};

const truncateSingleLine = (text: string, maxLength = 120): string => {
  const normalized = String(text || '').replace(/\s+/g, ' ').trim();
  if (!normalized) return '';
  if (normalized.length <= maxLength) return normalized;
  return `${normalized.slice(0, maxLength)}...`;
};

const truncateByChars = (text: string, maxChars: number): { value: string; truncated: boolean } => {
  if (maxChars <= 0) return { value: '', truncated: text.length > 0 };
  const chars = Array.from(String(text || ''));
  if (chars.length <= maxChars) return { value: chars.join(''), truncated: false };
  return { value: chars.slice(0, maxChars).join(''), truncated: true };
};

const buildTextPreview = (
  text: string,
  maxLines = 8,
  maxChars = 900,
  indent = '  '
): string => {
  const normalized = String(text || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n').trim();
  if (!normalized) return '';

  const { value, truncated } = truncateByChars(normalized, maxChars);
  const parts = value.split('\n');
  const visible = parts.slice(0, Math.max(maxLines, 1));
  const hiddenLines = Math.max(parts.length - visible.length, 0);

  const rows = visible.map((line, index) => (index === 0 ? line : `${indent}${line}`));
  if (hiddenLines > 0 || truncated) {
    const extras: string[] = [];
    if (hiddenLines > 0) extras.push(`${hiddenLines} more lines`);
    if (truncated) extras.push('truncated');
    rows.push(`${indent}... (${extras.join(', ')})`);
  }
  return rows.join('\n');
};

const buildLabeledTextBlock = (
  rows: Array<{ label: string; value: unknown }>,
  separator = ': '
): string =>
  rows
    .map(({ label, value }) => {
      const text = String(value ?? '')
        .trim()
        .replace(/\s+/g, ' ');
      if (!label || !text) return '';
      return `${label}${separator}${text}`;
    })
    .filter(Boolean)
    .join('\n');

const buildBulletListBlock = (items: string[], limit = 6, maxChars = 180): string => {
  const normalized = items.map((item) => String(item || '').trim()).filter(Boolean);
  if (!normalized.length) return '';
  const visible = normalized.slice(0, limit).map((item) => `- ${truncateSingleLine(item, maxChars)}`);
  if (normalized.length > visible.length) {
    visible.push(`... (+${normalized.length - visible.length})`);
  }
  return visible.join('\n');
};

const normalizeTerminalAutoStickMode = (value: unknown): TerminalAutoStickMode => {
  const normalized = String(value || '')
    .trim()
    .toLowerCase();
  if (normalized === 'always' || normalized === 'off') return normalized;
  return 'smart';
};

const normalizeMultiline = (text: string): string =>
  String(text || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');

const truncateByMiddle = (
  text: string,
  maxChars: number
): { value: string; truncated: boolean } => {
  if (maxChars <= 0) return { value: '', truncated: text.length > 0 };
  const chars = Array.from(text);
  if (chars.length <= maxChars) return { value: chars.join(''), truncated: false };
  const headChars = Math.max(1, Math.floor(maxChars * 0.58));
  const tailChars = Math.max(1, maxChars - headChars);
  const omitted = Math.max(chars.length - headChars - tailChars, 0);
  const head = chars.slice(0, headChars).join('');
  const tail = chars.slice(chars.length - tailChars).join('');
  return {
    value: `${head}\n... (${omitted} chars omitted)\n${tail}`,
    truncated: true
  };
};

const buildTerminalStream = (
  text: string,
  _status: string,
  maxLines = 260,
  maxChars = 16000
): string => {
  const normalized = normalizeMultiline(text);
  if (!normalized.trim()) return '';

  const lines = normalized.split('\n');
  const keepLines = Math.max(maxLines, 1);
  let lineText = normalized;
  let hiddenLines = 0;
  if (lines.length > keepLines) {
    const headLines = Math.max(1, Math.floor(keepLines * 0.62));
    const tailLines = Math.max(1, keepLines - headLines);
    hiddenLines = Math.max(lines.length - headLines - tailLines, 0);
    const head = lines.slice(0, headLines);
    const tail = lines.slice(lines.length - tailLines);
    lineText = [...head, `... (${hiddenLines} lines omitted)`, ...tail].join('\n');
  }

  const { value, truncated } = truncateByMiddle(lineText, maxChars);
  if (!hiddenLines && !truncated) return value;
  return value;
};

const isApplyPatchTool = (toolName: string): boolean => {
  const normalized = toolName.trim().toLowerCase();
  return normalized === 'apply_patch' || toolName.includes('应用补丁');
};

const isExecuteCommandTool = (toolName: string): boolean => {
  const normalized = toolName.trim().toLowerCase();
  return normalized === 'execute_command' || toolName.includes('执行命令');
};

const isReadFileTool = (toolName: string): boolean => {
  const normalized = toolName.trim().toLowerCase();
  return normalized === 'read_file' || toolName.includes('读取文件');
};

const isListFilesTool = (toolName: string): boolean => {
  const normalized = toolName.trim().toLowerCase();
  return normalized === 'list_files' || toolName.includes('列出文件');
};

const isSearchContentTool = (toolName: string): boolean => {
  const normalized = toolName.trim().toLowerCase();
  return normalized === 'search_content' || toolName.includes('搜索内容');
};

const isWriteFileTool = (toolName: string): boolean => {
  const normalized = toolName.trim().toLowerCase();
  return normalized === 'write_file' || toolName.includes('写入文件');
};

const isCompactionTool = (toolName: string): boolean => toolName.trim() === '上下文压缩';

const extractToolResultObject = (detailObject: UnknownObject | null): UnknownObject | null => {
  if (!detailObject) return null;
  return asObject(detailObject.result) || detailObject;
};

const extractToolResultData = (resultObject: UnknownObject | null): UnknownObject | null => {
  if (!resultObject) return null;
  return asObject(resultObject.data) || resultObject;
};

const resolveToolName = (item: WorkflowItem): string => {
  const direct = String(item.toolName || '').trim();
  if (direct) return direct;
  const rawTitle = String(item.title || '').trim();
  return rawTitle
    .replace(/^调用工具[:：]\s*/i, '')
    .replace(/^工具结果[:：]\s*/i, '')
    .replace(/^工具输出[:：]\s*/i, '')
    .replace(/^Tool\s+call:\s*/i, '')
    .replace(/^Tool\s+result:\s*/i, '')
    .replace(/^Tool\s+output:\s*/i, '')
    .trim();
};

const resolveToolEventKind = (item: WorkflowItem): 'call' | 'output' | 'result' | null => {
  const eventType = String(item.eventType || '').trim().toLowerCase();
  if (eventType === 'tool_call') return 'call';
  if (eventType === 'tool_output_delta') return 'output';
  if (eventType === 'compaction_progress') return 'output';
  if (eventType === 'tool_result') return 'result';
  if (eventType === 'compaction') return 'result';

  const title = String(item.title || '').trim();
  if (/^调用工具[:：]/i.test(title) || /^Tool\s+call:/i.test(title)) return 'call';
  if (/^工具输出[:：]/i.test(title) || /^Tool\s+output:/i.test(title) || title === '工具输出') {
    return 'output';
  }
  if (/^工具结果[:：]/i.test(title) || /^Tool\s+result:/i.test(title)) return 'result';

  return item.isTool ? 'result' : null;
};

const extractFirstCommandLine = (text: unknown): string => {
  if (typeof text !== 'string') return '';
  const normalized = text.replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  const firstLine = normalized
    .split('\n')
    .map((line) => line.trim())
    .find(Boolean);
  return firstLine ? truncateSingleLine(firstLine, 180) : '';
};

const resolveCommandFromCall = (item: WorkflowItem | null): string => {
  if (!item) return '';
  const detailObject = parseDetailObject(item.detail);
  const args = extractCallArgs(item);
  return pickString(
    args?.command,
    extractFirstCommandLine(args?.content),
    extractFirstCommandLine(args?.input),
    extractFirstCommandLine(args?.raw),
    args?.cmd,
    args?.script,
    detailObject?.command,
    extractFirstCommandLine(detailObject?.content),
    extractFirstCommandLine(detailObject?.input),
    detailObject?.cmd,
    detailObject?.script
  );
};

const decodeEscapedText = (raw: string): string => {
  const value = String(raw || '');
  if (!value) return '';
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value
      .replace(/\\n/g, '\n')
      .replace(/\\r/g, '\r')
      .replace(/\\t/g, '\t')
      .replace(/\\"/g, '"')
      .replace(/\\\\/g, '\\');
  }
};

const emptyCommandRecord = (): CommandRecord => ({
  command: '',
  stdout: '',
  stderr: '',
  returncode: null
});

const extractCommandRecordFromObject = (root: UnknownObject | null): CommandRecord => {
  if (!root) return emptyCommandRecord();
  const data = asObject(root.data) || root;
  const firstResult = Array.isArray(data?.results)
    ? (data.results.find((value) => asObject(value)) as UnknownObject | undefined)
    : undefined;
  const source = firstResult || data;

  return {
    command: pickString(
      source?.command,
      data?.command,
      root.command,
      extractFirstCommandLine(source?.content),
      extractFirstCommandLine(source?.input),
      extractFirstCommandLine(data?.content),
      extractFirstCommandLine(data?.input),
      extractFirstCommandLine(root.content),
      extractFirstCommandLine(root.input)
    ),
    stdout: pickString(
      source?.stdout,
      source?.output,
      source?.result,
      data?.stdout,
      data?.output,
      data?.result,
      root.stdout,
      root.output,
      root.result
    ),
    stderr: pickString(
      source?.stderr,
      source?.error,
      data?.stderr,
      data?.error,
      root.stderr,
      root.error
    ),
    returncode: toOptionalInt(source?.returncode, data?.returncode, root.returncode)
  };
};

const extractCommandRecordFromUnknown = (candidate: unknown): CommandRecord => {
  const directObject = asObject(candidate);
  if (directObject) return extractCommandRecordFromObject(directObject);
  if (typeof candidate === 'string') {
    const parsedDirect = parseDetailObject(candidate);
    if (parsedDirect) return extractCommandRecordFromObject(parsedDirect);
    const decoded = decodeEscapedText(candidate);
    const parsedDecoded = parseDetailObject(decoded);
    if (parsedDecoded) return extractCommandRecordFromObject(parsedDecoded);
  }
  return emptyCommandRecord();
};

const extractPreviewField = (preview: string, field: string): string => {
  if (!preview) return '';
  const patterns = [
    new RegExp(`"${field}"\\s*:\\s*"((?:\\\\.|[^"\\\\])*)"`, 'i'),
    new RegExp(`\\\\"${field}\\\\"\\s*:\\s*\\\\"([\\s\\S]*?)(?:\\\\"\\s*,\\s*\\\\"|\\\\"\\s*[}\\]])`, 'i'),
    new RegExp(`"${field}"\\s*:\\s*"([\\s\\S]+)$`, 'i')
  ];
  for (const pattern of patterns) {
    const match = preview.match(pattern);
    if (match?.[1]) return decodeEscapedText(match[1]);
  }
  return '';
};

const extractCompactedCommandPayload = (
  resultObject: UnknownObject | null,
  dataObject: UnknownObject | null
): { preview: string; command: string; stdout: string; stderr: string; returncode: number | null } => {
  const preview = pickString(
    dataObject?.preview,
    resultObject?.preview,
    dataObject?.result_preview,
    resultObject?.result_preview
  );
  if (!preview) return { preview: '', command: '', stdout: '', stderr: '', returncode: null };

  const parsed = extractCommandRecordFromUnknown(preview);
  const command = pickString(parsed.command, extractPreviewField(preview, 'command'));
  const stdout = pickString(parsed.stdout, extractPreviewField(preview, 'stdout'));
  const stderr = pickString(parsed.stderr, extractPreviewField(preview, 'stderr'));
  const returncode = toOptionalInt(parsed.returncode, extractPreviewField(preview, 'returncode'));
  return { preview, command, stdout, stderr, returncode };
};

const resolveCommandFromResult = (item: WorkflowItem | null): string => {
  if (!item) return '';
  const detailObject = parseDetailObject(item.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);
  const firstResult = Array.isArray(dataObject?.results)
    ? (dataObject.results.find((value) => asObject(value)) as UnknownObject | undefined)
    : undefined;
  const compacted = extractCompactedCommandPayload(resultObject, dataObject);
  return pickString(
    firstResult?.command,
    dataObject?.command,
    resultObject?.command,
    compacted.command
  );
};

const extractTaggedSection = (detail: string, tag: 'command' | 'stdout' | 'stderr'): string => {
  const normalized = String(detail || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  const pattern = new RegExp(
    `\\[${tag}\\]\\n([\\s\\S]*?)(?=\\n\\n\\[(?:command|stdout|stderr)\\]\\n|$)`,
    'i'
  );
  const match = normalized.match(pattern);
  return match?.[1]?.trim() || '';
};

const resolveCommandFromOutput = (item: WorkflowItem | null): string => {
  if (!item?.detail) return '';
  return pickString(extractTaggedSection(item.detail, 'command'));
};

const extractToolOutputStreams = (
  outputItem: WorkflowItem | null
): { command: string; stdout: string; stderr: string } => {
  const detail = String(outputItem?.detail || '').trim();
  if (!detail) {
    return { command: '', stdout: '', stderr: '' };
  }
  return {
    command: extractTaggedSection(detail, 'command'),
    stdout: extractTaggedSection(detail, 'stdout'),
    stderr: extractTaggedSection(detail, 'stderr')
  };
};

const extractCallArgs = (item: WorkflowItem | null): UnknownObject | null => {
  if (!item) return null;
  const detailObject = parseDetailObject(item.detail);
  if (!detailObject) return null;
  const nestedFunction = asObject(detailObject.function);
  const candidates: unknown[] = [detailObject.args, detailObject.arguments, nestedFunction?.arguments];
  for (const candidate of candidates) {
    const directObject = asObject(candidate);
    if (directObject) return directObject;
    if (typeof candidate === 'string') {
      const parsed = parseDetailObject(candidate);
      if (parsed) return parsed;
    }
  }
  return detailObject;
};

const appendPathCandidate = (target: Set<string>, value: unknown) => {
  if (typeof value !== 'string') return;
  const text = value.trim();
  if (!text) return;
  target.add(text);
};

const appendPathFromObject = (target: Set<string>, obj: UnknownObject | null) => {
  if (!obj) return;
  PATH_HINT_KEYS.forEach((key) => appendPathCandidate(target, obj[key]));
};

const collectPathHintsFromArgs = (args: UnknownObject | null, limit = FILE_HINT_LIMIT): string[] => {
  if (!args) return [];
  const hints = new Set<string>();
  appendPathFromObject(hints, args);

  for (const key of ['files', 'paths', 'targets', 'inputs', 'outputs', 'edits']) {
    const value = args[key];
    if (Array.isArray(value)) {
      value.forEach((item) => {
        if (typeof item === 'string') {
          appendPathCandidate(hints, item);
          return;
        }
        appendPathFromObject(hints, asObject(item));
      });
      continue;
    }
    appendPathCandidate(hints, value);
  }

  return Array.from(hints).slice(0, limit);
};

const collectPathHintsFromResult = (
  resultItem: WorkflowItem | null,
  limit = FILE_HINT_LIMIT
): string[] => {
  if (!resultItem) return [];
  const hints = new Set<string>();
  const detailObject = parseDetailObject(resultItem.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);
  appendPathFromObject(hints, resultObject);
  appendPathFromObject(hints, dataObject);
  if (Array.isArray(dataObject?.results)) {
    dataObject.results.forEach((item) => appendPathFromObject(hints, asObject(item)));
  }
  return Array.from(hints).slice(0, limit);
};

const splitMoveText = (text: string): string[] => {
  const normalized = String(text || '').trim();
  if (!normalized) return [];
  const parts = normalized
    .split(/\s*->\s*/g)
    .map((item) => item.trim())
    .filter(Boolean);
  if (parts.length > 1) return parts;
  return [normalized];
};

const resolvePatchInput = (callItem: WorkflowItem | null): string => {
  const args = extractCallArgs(callItem);
  return pickString(args?.input, args?.patch, args?.content, args?.raw);
};

function buildCompactPatchPathLabel(path: string, toPath = ''): string {
  const compactSingle = (value: string): string => {
    const normalized = normalizePathLikeText(value);
    if (!normalized) return '';
    const shortened = shortenPathLike(normalized, 2) || normalized;
    const basename = basenameOfPathLike(normalized);
    const parentLabel = shortened.replace(/^\.\.\//, '');
    if (!basename || basename.length > 36) return shortened;
    if (!parentLabel || parentLabel === basename || parentLabel.endsWith(`/${basename}`)) {
      return basename;
    }
    return shortened;
  };

  const left = compactSingle(path);
  const right = compactSingle(toPath);
  const normalizedLeft = normalizePathLikeText(path);
  const normalizedRight = normalizePathLikeText(toPath);
  if (left && right && normalizedLeft && normalizedRight && normalizedLeft !== normalizedRight) {
    return `${left} -> ${right}`;
  }
  return left || right;
}

const buildApplyPatchEntries = (item: WorkflowItem | null, toolName: string): PatchEntry[] => {
  if (!item || !isApplyPatchTool(toolName)) return [];
  const detailObject = parseDetailObject(item.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);
  const files = Array.isArray(dataObject?.files) ? (dataObject.files as unknown[]) : [];
  const entries: PatchEntry[] = [];

  for (let index = 0; index < Math.min(files.length, PATCH_RESULT_FILE_LIMIT); index += 1) {
    const fileObject = asObject(files[index]);
    if (!fileObject) continue;
    const action = String(fileObject.action || '').trim().toLowerCase();
    const path = String(fileObject.path || '').trim();
    const toPath = String(fileObject.to_path || '').trim();
    if (!path && !toPath) continue;

    let kind: PatchEntry['kind'] = 'other';
    let sign = '~';
    if (action === 'add') {
      kind = 'add';
      sign = '+';
    } else if (action === 'delete') {
      kind = 'delete';
      sign = '-';
    } else if (path && toPath && path !== toPath) {
      kind = 'move';
      sign = '>';
    } else {
      kind = 'update';
      sign = '~';
    }

    entries.push({
      key: `${String(item.id || 'patch')}-${index}`,
      kind,
      sign,
      text: buildCompactPatchPathLabel(path, toPath) || (path && toPath && path !== toPath ? `${path} -> ${toPath}` : path || toPath)
    });
  }

  if (files.length > entries.length) {
    entries.push({
      key: `${String(item.id || 'patch')}-more`,
      kind: 'other',
      sign: '...',
      text: t('chat.toolWorkflow.moreFiles', { count: files.length - entries.length })
    });
  }

  return entries;
};

const normalizePatchPreviewLine = (line: string): string => {
  const normalized = String(line || '').replace(/\t/g, '  ');
  if (normalized.length <= PATCH_PREVIEW_LINE_MAX_CHARS) return normalized;
  return `${normalized.slice(0, PATCH_PREVIEW_LINE_MAX_CHARS)}...`;
};

const parseApplyPatchPreview = (patchText: string): RawPatchPreview[] => {
  const normalized = String(patchText || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  if (!normalized.includes('*** Begin Patch')) return [];

  const previews: RawPatchPreview[] = [];
  const rows = normalized.split('\n');
  let current: RawPatchPreview | null = null;

  const flush = () => {
    if (current && (current.path || current.toPath)) {
      previews.push(current);
    }
    current = null;
  };

  const pushLine = (line: string) => {
    if (!current) return;
    if (current.lines.length < PATCH_PREVIEW_LINE_LIMIT) {
      current.lines.push(normalizePatchPreviewLine(line));
      return;
    }
    current.omitted += 1;
  };

  for (const row of rows) {
    const addMatch = row.match(/^\*\*\* Add File:\s*(.+)\s*$/);
    if (addMatch) {
      flush();
      current = { action: 'add', path: addMatch[1].trim(), toPath: '', lines: [], omitted: 0 };
      continue;
    }

    const updateMatch = row.match(/^\*\*\* Update File:\s*(.+)\s*$/);
    if (updateMatch) {
      flush();
      current = { action: 'update', path: updateMatch[1].trim(), toPath: '', lines: [], omitted: 0 };
      continue;
    }

    const deleteMatch = row.match(/^\*\*\* Delete File:\s*(.+)\s*$/);
    if (deleteMatch) {
      flush();
      current = { action: 'delete', path: deleteMatch[1].trim(), toPath: '', lines: [], omitted: 0 };
      continue;
    }

    const moveMatch = row.match(/^\*\*\* Move to:\s*(.+)\s*$/);
    if (moveMatch) {
      if (current) {
        current.toPath = moveMatch[1].trim();
        if (current.action === 'update') current.action = 'move';
      }
      continue;
    }

    if (row.startsWith('*** End Patch')) {
      flush();
      break;
    }
    if (!current) continue;

    if (row.startsWith('+') && !row.startsWith('+++')) {
      pushLine(`+${row.slice(1)}`);
      continue;
    }
    if (row.startsWith('-') && !row.startsWith('---')) {
      pushLine(`-${row.slice(1)}`);
      continue;
    }
    if (row.startsWith('@@')) {
      pushLine(`@@ ${row.slice(2).trim()}`);
    }
  }

  flush();
  return previews;
};

const buildApplyPatchDiffBlocks = (callItem: WorkflowItem | null, toolName: string): PatchDiffBlock[] => {
  if (!isApplyPatchTool(toolName)) return [];
  const patchInput = resolvePatchInput(callItem);
  if (!patchInput) return [];

  const previews = parseApplyPatchPreview(patchInput);
  return previews.slice(0, PATCH_PREVIEW_FILE_LIMIT).map((preview, index) => {
    const pathText =
      preview.path && preview.toPath && preview.path !== preview.toPath
        ? `${preview.path} -> ${preview.toPath}`
        : preview.path || preview.toPath;
    const lines: PatchDiffLine[] = [];

    preview.lines.forEach((line, lineIndex) => {
      let kind: PatchDiffLine['kind'] = 'meta';
      if (line.startsWith('+')) kind = 'add';
      else if (line.startsWith('-')) kind = 'delete';
      lines.push({
        key: `line-${index}-${lineIndex}`,
        kind,
        text: line
      });
    });

    if (lines.length === 0) {
      lines.push({
        key: `line-${index}-empty`,
        kind: 'meta',
        text: '~ (no inline diff preview)'
      });
    }

    if (preview.omitted > 0) {
      lines.push({
        key: `line-${index}-omit`,
        kind: 'omit',
        text: `... (+${preview.omitted})`
      });
    }

    return {
      key: `diff-${index}`,
      title: buildCompactPatchPathLabel(preview.path, preview.toPath) || pathText || `file-${index + 1}`,
      pathHint: pathText,
      lines
    };
  });
};

const formatTimeoutValue = (...values: unknown[]): string => {
  for (const value of values) {
    if (typeof value === 'number' && Number.isFinite(value) && value > 0) {
      return `${Number.isInteger(value) ? value : value.toFixed(value >= 10 ? 0 : 1)}s`;
    }
    if (typeof value !== 'string') continue;
    const trimmed = value.trim();
    if (!trimmed) continue;
    const parsed = Number(trimmed);
    if (Number.isFinite(parsed) && parsed > 0) {
      return `${Number.isInteger(parsed) ? parsed : parsed.toFixed(parsed >= 10 ? 0 : 1)}s`;
    }
    return trimmed;
  }
  return '';
};

const countNonEmptyCommandLines = (value: string): number =>
  String(value || '')
    .replace(/\r\n/g, '\n')
    .replace(/\r/g, '\n')
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean).length;

const formatByteCountLabel = (value: number | null): string => {
  if (value === null || value < 0) return '';
  if (value < 1024) return `${value} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let size = value;
  let unitIndex = -1;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  return `${size.toFixed(size >= 10 ? 0 : 1)} ${units[Math.max(unitIndex, 0)]}`;
};

const countApplyPatchFiles = (patchInput: string, fallback = 0): number => {
  if (!patchInput) return fallback;
  return patchInput.match(/^\*\*\* (?:Add|Update|Delete) File:/gm)?.length || fallback;
};

const countApplyPatchHunks = (patchInput: string, fallback = 0): number => {
  if (!patchInput) return fallback;
  return patchInput.match(/^@@/gm)?.length || fallback;
};

// Keep command/patch sections consistent: summary shows metadata, body keeps raw payload.
const buildExecuteCommandCallSummary = (entry: RawEntry): string => {
  const args = extractCallArgs(entry.callItem);
  const commandText = pickString(
    args?.content,
    args?.input,
    args?.raw,
    args?.script,
    args?.command,
    args?.cmd,
    resolveCommandFromCall(entry.callItem)
  );
  const commandLine = truncateSingleLine(resolveCommandFromCall(entry.callItem), 120);
  const commandCount = Math.max(countNonEmptyCommandLines(commandText), commandLine ? 1 : 0);
  const workdir = pickString(args?.workdir, args?.cwd, args?.dir, args?.directory);
  const timeout = formatTimeoutValue(
    args?.timeout_s,
    args?.timeout,
    args?.timeoutSeconds,
    args?.timeout_seconds
  );

  return buildLabeledTextBlock([
    {
      label:
        commandCount > 1
          ? t('chat.toolWorkflow.detail.commands')
          : t('chat.toolWorkflow.detail.command'),
      value: commandCount > 1 ? commandCount : commandLine
    },
    { label: t('chat.toolWorkflow.detail.workdir'), value: workdir },
    { label: t('chat.toolWorkflow.detail.timeout'), value: timeout }
  ]);
};

const buildExecuteCommandCallBody = (entry: RawEntry, command: string): string => {
  const args = extractCallArgs(entry.callItem);
  const commandText = pickString(
    args?.content,
    args?.input,
    args?.raw,
    args?.script,
    args?.command,
    args?.cmd,
    command
  );

  const extraArgs = omitObjectKeys(args, [
    'command',
    'cmd',
    'script',
    'raw',
    'content',
    'input',
    'workdir',
    'cwd',
    'dir',
    'directory',
    'timeout_s',
    'timeout',
    'timeoutSeconds',
    'timeout_seconds'
  ]);
  const extraPreview = buildObjectPreview(extraArgs);
  const blocks: string[] = [];
  if (commandText && (countNonEmptyCommandLines(commandText) > 1 || !extraPreview)) {
    if (countNonEmptyCommandLines(commandText) > 1) {
      blocks.push(buildTerminalStream(commandText, 'completed', 120, 16000));
    }
  }
  if (extraPreview) blocks.push(extraPreview);
  return blocks.join('\n\n').trim();
};

const buildExecuteCommandResultSummary = (entry: RawEntry): string => {
  const { resultObject, dataObject } = extractResultPayload(entry.resultItem);
  const resultMeta = asObject(resultObject?.meta);
  const dataMeta = asObject(dataObject?.meta);
  const outputGuard =
    asObject(resultMeta?.output_guard) || asObject(dataMeta?.output_guard) || asObject(dataObject?.output_guard);
  const resultList = Array.isArray(dataObject?.results)
    ? (dataObject.results.map((item) => asObject(item)).filter(Boolean) as UnknownObject[])
    : [];
  const commandLine = truncateSingleLine(
    pickString(
      resultList[0]?.command,
      dataObject?.command,
      resultObject?.command,
      resolveCommandFromOutput(entry.outputItem),
      resolveCommandFromResult(entry.resultItem)
    ),
    120
  );
  const commandCount =
    toOptionalInt(outputGuard?.commands) || resultList.length || (commandLine ? 1 : 0);
  const exitCode = resolveExecuteCommandExitCode(resultObject, dataObject);
  const truncatedCommands = toOptionalInt(outputGuard?.truncated_commands);
  const totalBytes = toOptionalInt(outputGuard?.total_bytes);
  const omittedBytes = toOptionalInt(outputGuard?.omitted_bytes);

  return buildLabeledTextBlock([
    {
      label:
        commandCount > 1
          ? t('chat.toolWorkflow.detail.commands')
          : t('chat.toolWorkflow.detail.command'),
      value: commandCount > 1 ? commandCount : commandLine
    },
    { label: t('chat.toolWorkflow.detail.exitCode'), value: exitCode === null ? '' : exitCode },
    {
      label: t('chat.toolWorkflow.detail.truncatedCommands'),
      value: truncatedCommands && truncatedCommands > 0 ? truncatedCommands : ''
    },
    { label: t('chat.toolWorkflow.detail.totalBytes'), value: formatByteCountLabel(totalBytes) },
    { label: t('chat.toolWorkflow.detail.omittedBytes'), value: formatByteCountLabel(omittedBytes) }
  ]);
};

const buildApplyPatchCallSummary = (entry: RawEntry, patchDiffBlocks: PatchDiffBlock[]): string => {
  const patchInput = resolvePatchInput(entry.callItem);
  const fallbackHunks = patchDiffBlocks.reduce(
    (count, block) => count + block.lines.filter((line) => line.text.startsWith('@@')).length,
    0
  );
  return buildLabeledTextBlock([
    {
      label: t('chat.toolWorkflow.detail.changedFiles'),
      value: countApplyPatchFiles(patchInput, patchDiffBlocks.length) || ''
    },
    {
      label: t('chat.toolWorkflow.detail.hunks'),
      value: countApplyPatchHunks(patchInput, fallbackHunks) || ''
    }
  ]);
};

const buildApplyPatchResultSummary = (entry: RawEntry): string => {
  if (!entry.resultItem || !isApplyPatchTool(entry.toolName)) return '';
  const detailObject = parseDetailObject(entry.resultItem.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);

  const changedFiles = toInt(dataObject?.changed_files, resultObject?.changed_files);
  const hunksApplied = toInt(dataObject?.hunks_applied, resultObject?.hunks_applied);
  const added = toInt(dataObject?.added, resultObject?.added);
  const updated = toInt(dataObject?.updated, resultObject?.updated);
  const deleted = toInt(dataObject?.deleted, resultObject?.deleted);
  const moved = toInt(dataObject?.moved, resultObject?.moved);

  return buildLabeledTextBlock([
    { label: t('chat.toolWorkflow.detail.changedFiles'), value: changedFiles || '' },
    { label: t('chat.toolWorkflow.detail.hunks'), value: hunksApplied || '' },
    { label: t('chat.toolWorkflow.detail.added'), value: added || '' },
    { label: t('chat.toolWorkflow.detail.updated'), value: updated || '' },
    { label: t('chat.toolWorkflow.detail.deleted'), value: deleted || '' },
    { label: t('chat.toolWorkflow.detail.moved'), value: moved || '' }
  ]);
};

type ApplyPatchCounts = {
  changedFiles: number;
  hunks: number;
  added: number;
  updated: number;
  deleted: number;
  moved: number;
};

const resolveApplyPatchCounts = (entry: RawEntry, patchDiffBlocks: PatchDiffBlock[] = []): ApplyPatchCounts => {
  if (!entry.resultItem || !isApplyPatchTool(entry.toolName)) {
    const patchInput = resolvePatchInput(entry.callItem);
    const fallbackHunks = patchDiffBlocks.reduce(
      (count, block) => count + block.lines.filter((line) => line.text.startsWith('@@')).length,
      0
    );
    return {
      changedFiles: countApplyPatchFiles(patchInput, patchDiffBlocks.length),
      hunks: countApplyPatchHunks(patchInput, fallbackHunks),
      added: 0,
      updated: 0,
      deleted: 0,
      moved: 0
    };
  }

  const detailObject = parseDetailObject(entry.resultItem.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);
  return {
    changedFiles: toInt(dataObject?.changed_files, resultObject?.changed_files),
    hunks: toInt(dataObject?.hunks_applied, resultObject?.hunks_applied),
    added: toInt(dataObject?.added, resultObject?.added),
    updated: toInt(dataObject?.updated, resultObject?.updated),
    deleted: toInt(dataObject?.deleted, resultObject?.deleted),
    moved: toInt(dataObject?.moved, resultObject?.moved)
  };
};

const buildApplyPatchCallFiles = (patchDiffBlocks: PatchDiffBlock[]): PatchFileView[] =>
  patchDiffBlocks.map((block) => ({
    key: block.key,
    title: block.title,
    meta: block.pathHint && block.pathHint !== block.title ? block.pathHint : '',
    lines: block.lines.map((line): PatchLine => {
      const kind: PatchLine['kind'] = line.kind === 'omit' ? 'note' : line.kind;
      return {
        key: line.key,
        kind,
        text: line.text
      };
    })
  }));

const resolvePatchEntryMeta = (entry: PatchEntry): string => {
  if (entry.kind === 'add') return t('chat.toolWorkflow.detail.added');
  if (entry.kind === 'delete') return t('chat.toolWorkflow.detail.deleted');
  if (entry.kind === 'move') return t('chat.toolWorkflow.detail.moved');
  if (entry.kind === 'update') return t('chat.toolWorkflow.detail.updated');
  return '';
};

const resolvePatchEntryTone = (
  entry: PatchEntry
): 'default' | 'success' | 'warning' | 'danger' => {
  if (entry.kind === 'add') return 'success';
  if (entry.kind === 'delete') return 'warning';
  return 'default';
};

const buildApplyPatchResultFiles = (patchEntries: PatchEntry[], errorText: string): PatchFileView[] => {
  const files: PatchFileView[] = patchEntries.map((entry) => ({
    key: entry.key,
    title: entry.text,
    meta: resolvePatchEntryMeta(entry),
    lines: [],
    tone: resolvePatchEntryTone(entry)
  }));
  if (errorText) {
    files.push({
      key: 'patch-error',
      title: 'error',
      meta: '',
      lines: [{ key: 'patch-error-line', kind: 'error', text: `error: ${errorText}` }],
      tone: 'danger'
    });
  }
  return files;
};

const extractDurationMs = (entry: RawEntry): number | null => {
  const detailObject = parseDetailObject(entry.resultItem?.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);
  return toOptionalInt(
    asObject(resultObject?.meta)?.duration_ms,
    dataObject?.duration_ms,
    dataObject?.elapsed_ms,
    dataObject?.durationMs,
    dataObject?.elapsedMs
  );
};

const toBool = (...values: unknown[]): boolean | null => {
  for (const value of values) {
    if (typeof value === 'boolean') return value;
    if (typeof value === 'string') {
      const normalized = value.trim().toLowerCase();
      if (normalized === 'true') return true;
      if (normalized === 'false') return false;
    }
  }
  return null;
};

const formatTokenTransition = (before: number | null, after: number | null): string => {
  if (before === null && after === null) return '';
  if (before !== null && after !== null) return `${before} → ${after} tokens`;
  if (before !== null) return `${before} tokens`;
  return `${after} tokens`;
};

const formatDurationLabel = (durationMs: number | null): string => {
  if (durationMs === null || durationMs <= 0) return '';
  if (durationMs < 1000) return `${durationMs}ms`;
  const seconds = durationMs / 1000;
  return seconds >= 10 ? `${seconds.toFixed(0)}s` : `${seconds.toFixed(1)}s`;
};

const collectEntryPathHints = (
  entry: RawEntry,
  patchEntries: PatchEntry[],
  patchDiffBlocks: PatchDiffBlock[]
): string[] => {
  const hints = new Set<string>();

  patchEntries.forEach((patch) => {
    splitMoveText(patch.text).forEach((item) => hints.add(item));
  });
  patchDiffBlocks.forEach((block) => {
    splitMoveText(block.pathHint).forEach((item) => hints.add(item));
  });
  collectPathHintsFromArgs(extractCallArgs(entry.callItem)).forEach((item) => hints.add(item));
  collectPathHintsFromResult(entry.resultItem).forEach((item) => hints.add(item));

  return Array.from(hints).slice(0, FILE_HINT_LIMIT);
};

const normalizePathLikeText = (value: unknown): string =>
  String(value || '')
    .trim()
    .replace(/\\/g, '/')
    .replace(/\/+/g, '/');

const basenameOfPathLike = (value: unknown): string => {
  const normalized = normalizePathLikeText(value).replace(/\/+$/, '');
  if (!normalized) return '';
  const segments = normalized.split('/').filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] : normalized;
};

const isPtcTool = (toolName: string): boolean => {
  const normalized = toolName.trim().toLowerCase();
  return normalized === 'ptc' || normalized === 'programmatic_tool_call';
};

const isSkillCallTool = (toolName: string): boolean => {
  const normalized = toolName.trim().toLowerCase();
  return normalized === 'skill_call' || normalized === 'skill_get' || toolName.includes('技能调用');
};

const resolveSummaryToolDisplay = (toolName: string, fallback: string): string => {
  if (isSkillCallTool(toolName)) return t('chat.toolWorkflow.toolLabel.skillCall');
  if (isPtcTool(toolName)) return t('chat.toolWorkflow.toolLabel.ptc');
  if (isExecuteCommandTool(toolName)) return t('chat.toolWorkflow.toolLabel.executeCommand');
  if (isApplyPatchTool(toolName)) return t('chat.toolWorkflow.toolLabel.applyPatch');
  if (isReadFileTool(toolName)) return t('chat.toolWorkflow.toolLabel.readFile');
  if (isWriteFileTool(toolName)) return t('chat.toolWorkflow.toolLabel.writeFile');
  if (isListFilesTool(toolName)) return t('chat.toolWorkflow.toolLabel.listFiles');
  if (isSearchContentTool(toolName)) return t('chat.toolWorkflow.toolLabel.searchContent');
  return fallback;
};

const formatContextWindowLabel = (before: unknown, after: unknown): string => {
  const beforeCount = toInt(before);
  const afterCount = toInt(after);
  if (beforeCount <= 0 && afterCount <= 0) return '';
  return `-${beforeCount} / +${afterCount}`;
};

const collectReadRangeLabels = (source: UnknownObject | null): string[] => {
  if (!source) return [];
  const labels: string[] = [];
  const pushRange = (startValue: unknown, endValue: unknown) => {
    const start = toOptionalInt(startValue);
    const end = toOptionalInt(endValue);
    if (start === null) return;
    labels.push(start === end || end === null ? `${start}` : `${start}-${end}`);
  };

  if (Array.isArray(source.line_ranges)) {
    source.line_ranges.forEach((item) => {
      if (!Array.isArray(item) || item.length < 2) return;
      pushRange(item[0], item[1]);
    });
  }
  if (labels.length === 0) {
    pushRange(source.start_line, source.end_line);
  }
  return labels;
};

const collectReadTargetLabels = (args: UnknownObject | null): string[] => {
  if (!args) return [];
  const specs = Array.isArray(args.files) && args.files.length > 0 ? args.files : [args];
  return specs
    .map((item) => asObject(item))
    .filter((item): item is UnknownObject => Boolean(item))
    .map((item) => {
      const path = pickString(item.path, item.file_path, item.file);
      const ranges = collectReadRangeLabels(item);
      if (!path) return '';
      return ranges.length > 0 ? `${path} (${ranges.join(', ')})` : path;
    })
    .filter(Boolean);
};

const splitPathLikeSegments = (value: unknown): string[] =>
  normalizePathLikeText(value)
    .replace(/^\/+/, '')
    .replace(/\/+$/, '')
    .split('/')
    .filter(Boolean);

const isLikelyFilePath = (value: unknown): boolean => {
  const basename = basenameOfPathLike(value);
  return /\.[a-z0-9_-]{1,16}$/i.test(basename);
};

const shortenPathLike = (value: unknown, segmentCount = 2): string => {
  const normalized = normalizePathLikeText(value);
  if (!normalized) return '';
  const segments = splitPathLikeSegments(normalized);
  if (segments.length === 0) return normalized;
  if (segments.length <= segmentCount) return segments.join('/');
  return `.../${segments.slice(-segmentCount).join('/')}`;
};

const isSamePathTail = (left: unknown, right: unknown): boolean => {
  const leftText = normalizePathLikeText(left).replace(/\/+$/, '');
  const rightText = normalizePathLikeText(right).replace(/\/+$/, '');
  if (!leftText || !rightText) return false;
  if (leftText === rightText) return true;
  return leftText.endsWith(`/${rightText}`) || rightText.endsWith(`/${leftText}`);
};

const isParentPathLike = (parent: unknown, child: unknown): boolean => {
  const parentText = normalizePathLikeText(parent).replace(/\/+$/, '');
  const childText = normalizePathLikeText(child).replace(/\/+$/, '');
  if (!parentText || !childText || parentText === childText) return false;
  return childText.startsWith(`${parentText}/`);
};

type SummaryPathCandidate = {
  raw: string;
  normalized: string;
  basename: string;
  isFile: boolean;
};

// Keep workflow titles compact by collapsing basename/full-path duplicates before display.
const buildSummaryPathLabels = (pathHints: string[]): string[] => {
  const candidates: SummaryPathCandidate[] = [];
  pathHints.forEach((hint) => {
    const normalized = normalizePathLikeText(hint);
    if (!normalized) return;
    if (candidates.some((item) => item.normalized === normalized)) return;
    candidates.push({
      raw: hint,
      normalized,
      basename: basenameOfPathLike(normalized),
      isFile: isLikelyFilePath(normalized)
    });
  });

  const selected: SummaryPathCandidate[] = [];
  candidates.forEach((candidate) => {
    if (selected.some((item) => isSamePathTail(item.normalized, candidate.normalized))) {
      return;
    }
    if (
      !candidate.isFile &&
      selected.some((item) => item.isFile && isParentPathLike(candidate.normalized, item.normalized))
    ) {
      return;
    }
    selected.push(candidate);
  });

  const basenameCount = new Map<string, number>();
  selected.forEach((candidate) => {
    if (!candidate.basename) return;
    basenameCount.set(candidate.basename, (basenameCount.get(candidate.basename) || 0) + 1);
  });

  const labels: string[] = [];
  selected.forEach((candidate) => {
    const label =
      candidate.basename && basenameCount.get(candidate.basename) === 1
        ? candidate.basename
        : shortenPathLike(candidate.normalized, candidate.isFile ? 2 : 2) || candidate.raw;
    if (!label || labels.includes(label)) return;
    labels.push(label);
  });
  return labels;
};

const normalizeSummaryPathLabel = (value: string): string => {
  const normalized = normalizePathLikeText(value);
  return normalized === '.' || normalized === './' ? '' : value;
};

const resolvePrimarySummaryPathLabel = (pathHints: string[]): string =>
  normalizeSummaryPathLabel(buildSummaryPathLabels(pathHints)[0] || '');

const countReadFileTargets = (entry: RawEntry): number => {
  const args = extractCallArgs(entry.callItem);
  if (Array.isArray(args?.files) && args.files.length > 0) {
    return args.files.length;
  }
  const { dataObject } = extractResultPayload(entry.resultItem);
  const meta = asObject(dataObject?.meta);
  if (Array.isArray(meta?.files) && meta.files.length > 0) {
    return meta.files.length;
  }
  return pickString(args?.path, args?.file, args?.file_path).trim() ? 1 : 0;
};

const formatCountSuffix = (count: number | null | undefined): string => {
  if (typeof count !== 'number' || !Number.isFinite(count) || count < 0) return '';
  return ` (${count})`;
};

const resolveListFilesSummaryTitle = (entry: RawEntry, toolDisplay: string, pathHints: string[]): string => {
  const { dataObject } = extractResultPayload(entry.resultItem);
  const itemCount = Array.isArray(dataObject?.items) ? dataObject.items.length : null;
  const pathLabel = resolvePrimarySummaryPathLabel(pathHints);
  return truncateSingleLine(`${toolDisplay}${pathLabel ? ` ${pathLabel}` : ''}${formatCountSuffix(itemCount)}`);
};

const resolveSearchContentSummaryTitle = (
  entry: RawEntry,
  toolDisplay: string,
  pathHints: string[]
): string => {
  const args = extractCallArgs(entry.callItem);
  const { dataObject } = extractResultPayload(entry.resultItem);
  const pathLabel = resolvePrimarySummaryPathLabel(pathHints);
  const queryLabel = truncateSingleLine(pickString(args?.query), 28);
  const hitCount = Array.isArray(dataObject?.hits)
    ? dataObject.hits.length
    : Array.isArray(dataObject?.matches)
      ? dataObject.matches.length
      : null;

  const quotedQuery = queryLabel ? `"${queryLabel}"` : '';
  const core = quotedQuery
    ? `${toolDisplay} ${quotedQuery}${pathLabel ? ` · ${pathLabel}` : ''}`
    : `${toolDisplay}${pathLabel ? ` ${pathLabel}` : ''}`;
  return truncateSingleLine(`${core}${formatCountSuffix(hitCount)}`);
};

const resolveReadFileSummaryTitle = (entry: RawEntry, toolDisplay: string, pathHints: string[]): string => {
  const pathLabel = resolvePrimarySummaryPathLabel(pathHints);
  const targetCount = countReadFileTargets(entry);
  const moreCount = Math.max(targetCount - (pathLabel ? 1 : 0), 0);
  return truncateSingleLine(`${toolDisplay}${pathLabel ? ` ${pathLabel}` : ''}${moreCount > 0 ? ` +${moreCount}` : ''}`);
};

const resolveWriteFileSummaryTitle = (toolDisplay: string, pathHints: string[]): string => {
  const pathLabel = resolvePrimarySummaryPathLabel(pathHints);
  return truncateSingleLine(`${toolDisplay}${pathLabel ? ` ${pathLabel}` : ''}`);
};

const resolveFileToolSummaryTitle = (entry: RawEntry, toolDisplay: string, pathHints: string[]): string => {
  if (isPtcTool(entry.toolName)) {
    return resolvePtcSummaryTitle(entry, toolDisplay, pathHints);
  }
  if (isWriteFileTool(entry.toolName)) {
    return resolveWriteFileSummaryTitle(toolDisplay, pathHints);
  }
  if (isReadFileTool(entry.toolName)) {
    return resolveReadFileSummaryTitle(entry, toolDisplay, pathHints);
  }
  if (isListFilesTool(entry.toolName)) {
    return resolveListFilesSummaryTitle(entry, toolDisplay, pathHints);
  }
  if (isSearchContentTool(entry.toolName)) {
    return resolveSearchContentSummaryTitle(entry, toolDisplay, pathHints);
  }
  return composeSummaryTitle(toolDisplay, pathHints);
};

const composeSummaryTitle = (base: string, pathHints: string[]): string => {
  const labels = buildSummaryPathLabels(pathHints);
  if (!labels.length) return truncateSingleLine(base);
  const visible = labels.slice(0, FILE_HINT_SUMMARY_LIMIT);
  const moreCount = Math.max(labels.length - visible.length, 0);
  const suffix = moreCount > 0 ? ` +${moreCount}` : '';
  return truncateSingleLine(`${base} ${visible.join(', ')}${suffix}`);
};

const resolvePtcSummaryTitle = (entry: RawEntry, toolDisplay: string, pathHints: string[]): string => {
  const args = extractCallArgs(entry.callItem);
  const { resultObject, dataObject } = extractResultPayload(entry.resultItem);
  const candidates = [
    args?.filename,
    args?.path,
    args?.file,
    dataObject?.path,
    resultObject?.path,
    ...pathHints
  ];
  const scriptName =
    candidates
      .map((value) => basenameOfPathLike(value))
      .find((value) => value && /\.py$/i.test(value)) ||
    candidates
      .map((value) => basenameOfPathLike(value))
      .find(Boolean) ||
    '';
  return truncateSingleLine(scriptName ? `${toolDisplay} ${scriptName}` : toolDisplay);
};

const resolveSkillPathName = (value: unknown): string => {
  const normalized = normalizePathLikeText(value);
  if (!normalized) return '';
  const segments = normalized.split('/').filter(Boolean);
  if (!segments.length) return '';
  const last = segments[segments.length - 1];
  if (!last) return '';
  if (last.toLowerCase() !== 'skill.md') {
    return basenameOfPathLike(last);
  }
  return segments.length >= 2 ? segments[segments.length - 2] : '';
};

const resolveSkillSummaryTitle = (entry: RawEntry, toolDisplay: string, pathHints: string[]): string => {
  const args = extractCallArgs(entry.callItem);
  const { resultObject, dataObject } = extractResultPayload(entry.resultItem);
  // Prefer the declared skill name from tool args/result, then fall back to folder name instead of generic SKILL.md.
  const skillName = pickString(
    dataObject?.name,
    resultObject?.name,
    args?.name,
    args?.skill_name,
    args?.skillName,
    resolveSkillPathName(dataObject?.root),
    resolveSkillPathName(resultObject?.root),
    resolveSkillPathName(dataObject?.path),
    resolveSkillPathName(resultObject?.path),
    ...pathHints.map((item) => resolveSkillPathName(item))
  );
  if (skillName) {
    return truncateSingleLine(`${toolDisplay} ${skillName}`);
  }
  return composeSummaryTitle(toolDisplay, pathHints);
};

const composeEntryTitle = (
  entry: RawEntry,
  toolDisplay: string,
  command: string,
  pathHints: string[]
): string => {
  if (command) {
    return truncateSingleLine(`${toolDisplay} ${command}`);
  }
  if (isSkillCallTool(entry.toolName)) {
    return resolveSkillSummaryTitle(entry, toolDisplay, pathHints);
  }
  return resolveFileToolSummaryTitle(entry, toolDisplay, pathHints);
};

const extractResultPayload = (
  resultItem: WorkflowItem | null
): { resultObject: UnknownObject | null; dataObject: UnknownObject | null } => {
  const detailObject = parseDetailObject(resultItem?.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);
  return { resultObject, dataObject };
};

const buildPreviewBlockWithCache = (
  detail: unknown,
  dataObject: UnknownObject | null,
  resultObject: UnknownObject | null
): string => {
  const detailKey = typeof detail === 'string' ? detail.trim() : '';
  if (detailKey) {
    const cached = getCachedPreview(detailKey);
    if (cached !== null) {
      return cached;
    }
  }
  const previewBlock = buildToolResultPreview(dataObject, resultObject);
  if (detailKey && previewBlock) {
    setCachedPreview(detailKey, previewBlock);
  }
  return previewBlock;
};

const parseReadFileSections = (content: string): Array<{ path: string; body: string }> => {
  const normalized = String(content || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  if (!normalized.trim()) return [];
  const chunks = normalized.split(/\n(?=>>> )/g);
  const sections: Array<{ path: string; body: string }> = [];
  chunks.forEach((chunk) => {
    const text = chunk.trim();
    if (!text.startsWith('>>> ')) return;
    const firstBreak = text.indexOf('\n');
    if (firstBreak < 0) {
      sections.push({ path: text.slice(4).trim(), body: '' });
      return;
    }
    sections.push({
      path: text.slice(4, firstBreak).trim(),
      body: text.slice(firstBreak + 1).trim()
    });
  });
  return sections;
};

const buildReadFileResultBlock = (dataObject: UnknownObject | null): string => {
  if (!dataObject) return '';
  const content = pickString(dataObject.content);
  if (!content) return '';
  const sections = parseReadFileSections(content);
  const meta = asObject(dataObject.meta);
  const metaFiles = Array.isArray(meta?.files)
    ? (meta.files.map((item) => asObject(item)).filter(Boolean) as UnknownObject[])
    : [];
  if (!sections.length) return buildTextPreview(content, 14, 1800, '');

  const fileBlocks = sections.slice(0, 3).map((section) => {
    const path = section.path || '(unknown)';
    const preview = buildTextPreview(section.body, 12, 1400, '');
    return `${path}\n${preview}`;
  });
  if (sections.length > fileBlocks.length) {
    fileBlocks.push(`... (+${sections.length - fileBlocks.length} files)`);
  }

  const summaryLines = metaFiles
    .slice(0, 3)
    .map((item) => {
      const path = pickString(item.path);
      const readLines = toInt(item.read_lines);
      const totalLines = toInt(item.total_lines);
      const binary = item.binary === true;
      if (!path) return '';
      if (binary) return `${path} · ${t('chat.toolWorkflow.detail.binary')}`;
      if (readLines > 0 && totalLines > 0) return `${path} · ${readLines}/${totalLines}`;
      if (totalLines > 0) return `${path} · ${totalLines}`;
      return path;
    })
    .filter(Boolean);
  const summaryBlock = summaryLines.length
    ? `${buildLabeledTextBlock([
        { label: t('chat.toolWorkflow.detail.files'), value: metaFiles.length || sections.length }
      ])}\n${buildBulletListBlock(summaryLines, 3, 120)}`
    : '';

  return [summaryBlock, fileBlocks.join('\n\n')].filter(Boolean).join('\n\n');
};

const buildListFilesResultBlock = (dataObject: UnknownObject | null): string => {
  const items = Array.isArray(dataObject?.items) ? (dataObject.items as unknown[]) : [];
  if (!items.length) return '';
  const normalized = items.map((item) => String(item || '').trim()).filter(Boolean);
  if (!normalized.length) return '';
  const metaBlock = buildLabeledTextBlock([
    { label: t('chat.toolWorkflow.detail.items'), value: normalized.length }
  ]);
  return [metaBlock, buildBulletListBlock(normalized, 30, 180)].filter(Boolean).join('\n\n');
};

const buildSearchResultBlock = (dataObject: UnknownObject | null): string => {
  const matches = Array.isArray(dataObject?.matches) ? (dataObject.matches as unknown[]) : [];
  if (!matches.length) return '';
  const normalized = matches.map((item) => String(item || '').trim()).filter(Boolean);
  if (!normalized.length) return '';
  const metaBlock = buildLabeledTextBlock([
    { label: t('chat.toolWorkflow.detail.hits'), value: normalized.length },
    { label: t('chat.toolWorkflow.detail.scannedFiles'), value: toInt(dataObject?.scanned_files) }
  ]);
  return [metaBlock, buildBulletListBlock(normalized, 24, 160)].filter(Boolean).join('\n\n');
};

const buildWriteFileResultBlock = (
  resultObject: UnknownObject | null,
  dataObject: UnknownObject | null
): string => {
  const firstResult = Array.isArray(dataObject?.results)
    ? (dataObject.results.find((value) => asObject(value)) as UnknownObject | undefined)
    : undefined;
  const path = pickString(
    firstResult?.path,
    firstResult?.file,
    firstResult?.file_path,
    dataObject?.path,
    dataObject?.file,
    dataObject?.file_path,
    resultObject?.path,
    resultObject?.file,
    resultObject?.file_path
  );
  const bytes = toInt(
    firstResult?.bytes,
    firstResult?.written_bytes,
    dataObject?.bytes,
    dataObject?.written_bytes,
    resultObject?.bytes,
    resultObject?.written_bytes
  );
  return buildLabeledTextBlock([
    { label: t('chat.toolWorkflow.detail.path'), value: path },
    { label: t('chat.toolWorkflow.detail.bytes'), value: bytes > 0 ? bytes : '' }
  ]);
};

const resolveExecuteCommandExitCode = (
  resultObject: UnknownObject | null,
  dataObject: UnknownObject | null
): number | null => {
  const firstResult = Array.isArray(dataObject?.results)
    ? (dataObject.results.find((value) => asObject(value)) as UnknownObject | undefined)
    : undefined;
  return toOptionalInt(
    firstResult?.returncode,
    dataObject?.returncode,
    resultObject?.returncode,
    resultObject?.meta && asObject(resultObject.meta)?.exit_code
  );
};

const looksStructuredResultText = (text: string): boolean => {
  const trimmed = String(text || '').trim();
  if (!trimmed) return false;
  if (!trimmed.startsWith('{') && !trimmed.startsWith('[')) return false;
  return (
    trimmed.includes('"results"') ||
    trimmed.includes('"stdout"') ||
    trimmed.includes('"stderr"') ||
    trimmed.includes('"returncode"')
  );
};

const normalizeCommandStreamText = (text: string, stream: 'stdout' | 'stderr'): string => {
  const raw = String(text || '');
  if (!raw.trim()) return '';
  const unwrapped = extractCommandRecordFromUnknown(raw);
  if (stream === 'stdout' && unwrapped.stdout) return unwrapped.stdout;
  if (stream === 'stderr' && unwrapped.stderr) return unwrapped.stderr;
  if (looksStructuredResultText(raw)) return '';
  return raw;
};

const stripBackendTruncationMarkers = (text: string): string =>
  String(text || '')
    .replace(/\.\.\.\(truncated\)\.\.\./gi, '')
    .replace(/\.\.\.\(truncated\)/gi, '')
    .replace(/\n{3,}/g, '\n\n')
    .trim();

const buildExecuteCommandTerminalText = (
  command: string,
  stdoutRaw: string,
  stderrRaw: string,
  previewRaw: string,
  errorText: string,
  status: string,
  includeCommandLine = true
): string => {
  const lines: string[] = [];
  if (includeCommandLine) {
    lines.push(`$ ${command || '(command)'}`);
  }

  const stdout = buildTerminalStream(stdoutRaw, status, 140, 18000);
  const stderr = buildTerminalStream(stderrRaw, status, 100, 12000);
  if (stdout) {
    lines.push(stdout);
  }
  if (stderr) {
    if (lines.length > 0) lines.push('');
    lines.push('[stderr]');
    lines.push(stderr);
  }

  const previewTrimmed = previewRaw.trim();
  const previewLooksLikeJson =
    (previewTrimmed.startsWith('{') || previewTrimmed.startsWith('[')) &&
    (previewTrimmed.includes('"results"') ||
      previewTrimmed.includes('"stdout"') ||
      previewTrimmed.includes('"returncode"'));

  if (!stdout && !stderr && previewRaw && !previewLooksLikeJson) {
    lines.push(buildTerminalStream(previewRaw, status, 320, 20000));
  }
  if (errorText) {
    if (lines.length > 0) lines.push('');
    lines.push(`error: ${errorText}`);
  }
  return lines.join('\n').trim();
};

const buildExecuteCommandView = (
  entry: RawEntry,
  command: string,
  status: string,
  errorText: string,
  includeCommandLine = true
): CommandView => {
  const args = extractCallArgs(entry.callItem);
  const { resultObject, dataObject } = extractResultPayload(entry.resultItem);
  const resultMeta = asObject(resultObject?.meta);
  const dataMeta = asObject(dataObject?.meta);
  const outputGuard =
    asObject(resultMeta?.output_guard) || asObject(dataMeta?.output_guard) || asObject(dataObject?.output_guard);
  const firstResult = Array.isArray(dataObject?.results)
    ? (dataObject.results.find((value) => asObject(value)) as UnknownObject | undefined)
    : undefined;
  const outputStreams = extractToolOutputStreams(entry.outputItem);
  const compacted = extractCompactedCommandPayload(resultObject, dataObject);
  const structuredCandidates = [
    extractCommandRecordFromUnknown(firstResult),
    extractCommandRecordFromUnknown(firstResult?.result),
    extractCommandRecordFromUnknown(firstResult?.output),
    extractCommandRecordFromUnknown(dataObject),
    extractCommandRecordFromUnknown(dataObject?.result),
    extractCommandRecordFromUnknown(dataObject?.output),
    extractCommandRecordFromUnknown(resultObject),
    extractCommandRecordFromUnknown(resultObject?.result),
    extractCommandRecordFromUnknown(resultObject?.output)
  ];
  const structuredCommand = pickString(...structuredCandidates.map((item) => item.command));
  const structuredStdout = pickString(...structuredCandidates.map((item) => item.stdout));
  const structuredStderr = pickString(...structuredCandidates.map((item) => item.stderr));
  const structuredReturncode = toOptionalInt(...structuredCandidates.map((item) => item.returncode));

  const resolvedCommand = pickString(
    command,
    outputStreams.command,
    firstResult?.command,
    extractFirstCommandLine(firstResult?.content),
    extractFirstCommandLine(firstResult?.input),
    dataObject?.command,
    extractFirstCommandLine(dataObject?.content),
    extractFirstCommandLine(dataObject?.input),
    resultObject?.command,
    structuredCommand,
    compacted.command
  );
  const exitCode = toOptionalInt(
    resolveExecuteCommandExitCode(resultObject, dataObject),
    structuredReturncode,
    compacted.returncode
  );
  const commandPayload = pickString(
    args?.content,
    args?.input,
    args?.raw,
    args?.script,
    args?.command,
    args?.cmd,
    resolvedCommand
  );
  const stdoutRaw = pickString(
    outputStreams.stdout,
    firstResult?.stdout,
    structuredStdout,
    compacted.stdout,
    dataObject?.stdout,
    resultObject?.stdout,
    firstResult?.output,
    firstResult?.result,
    dataObject?.output,
    dataObject?.result,
    resultObject?.output,
    resultObject?.result
  );
  const stderrRaw = pickString(
    outputStreams.stderr,
    firstResult?.stderr,
    structuredStderr,
    compacted.stderr,
    firstResult?.error,
    dataObject?.stderr,
    dataObject?.error,
    resultObject?.stderr,
    resultObject?.error
  );
  const normalizedStdout = normalizeCommandStreamText(stdoutRaw, 'stdout');
  const normalizedStderr = normalizeCommandStreamText(stderrRaw, 'stderr');
  const previewRaw = compacted.preview;
  const commandText = resolvedCommand || '(command)';
  const displayStdout = stripBackendTruncationMarkers(normalizedStdout);
  const displayStderr = stripBackendTruncationMarkers(normalizedStderr);
  const workdir = pickString(args?.workdir, args?.cwd, args?.dir, args?.directory);
  const timeout = formatTimeoutValue(
    args?.timeout_s,
    args?.timeout,
    args?.timeoutSeconds,
    args?.timeout_seconds
  );
  const commandCount = Math.max(
    countNonEmptyCommandLines(commandPayload),
    toOptionalInt(outputGuard?.commands) || 0,
    firstResult ? 1 : 0,
    commandText ? 1 : 0
  );
  const truncatedCommands = toOptionalInt(outputGuard?.truncated_commands);
  const totalBytes = formatByteCountLabel(toOptionalInt(outputGuard?.total_bytes));
  const omittedBytes = formatByteCountLabel(toOptionalInt(outputGuard?.omitted_bytes));

  const commandView = buildCommandCardView(
    {
      command: includeCommandLine ? commandText : commandText,
      shell: 'bash',
      exitCode,
      stdout: displayStdout,
      stderr: displayStderr,
      preview: previewRaw,
      workdir,
      timeout,
      commandCount,
      truncatedCommands,
      totalBytes,
      omittedBytes,
      errorText,
      showExitCode: false
    },
    t
  );

  return {
    ...commandView,
    terminalText: buildExecuteCommandTerminalText(
      commandText,
      displayStdout,
      displayStderr,
      previewRaw,
      errorText,
      status,
      includeCommandLine
    )
  };
};

const buildGenericResultBlock = (
  resultObject: UnknownObject | null,
  dataObject: UnknownObject | null,
  detail: unknown
): string => {
  if (!resultObject && !dataObject) return '';

  const headerRows: string[] = [];
  const path = pickString(dataObject?.path, resultObject?.path);
  if (path) headerRows.push(path);
  const bytes = toInt(dataObject?.bytes, resultObject?.bytes);
  if (bytes > 0) headerRows.push(`${bytes} bytes`);

  const summary = pickString(dataObject?.summary, resultObject?.summary, dataObject?.message, resultObject?.message);
  if (summary) headerRows.push(truncateSingleLine(summary, 180));

  const previewBlock = buildPreviewBlockWithCache(detail, dataObject, resultObject);
  const blocks: string[] = [];
  if (headerRows.length > 0) blocks.push(headerRows.join('\n'));
  if (previewBlock) blocks.push(previewBlock);
  return blocks.join('\n\n');
};

const buildResultBlock = (entry: RawEntry): string => {
  if (!entry.resultItem) return '';
  if (isApplyPatchTool(entry.toolName)) return '';
  if (isExecuteCommandTool(entry.toolName)) return '';

  const { resultObject, dataObject } = extractResultPayload(entry.resultItem);

  if (isReadFileTool(entry.toolName)) {
    return buildReadFileResultBlock(dataObject);
  }
  if (isListFilesTool(entry.toolName)) {
    return buildListFilesResultBlock(dataObject);
  }
  if (isSearchContentTool(entry.toolName)) {
    return buildSearchResultBlock(dataObject);
  }
  if (isWriteFileTool(entry.toolName)) {
    return buildWriteFileResultBlock(resultObject, dataObject);
  }
  return buildGenericResultBlock(resultObject, dataObject, entry.resultItem?.detail);
};

const buildOutputBlock = (entry: RawEntry, command: string): string => {
  if (isExecuteCommandTool(entry.toolName)) return '';
  const outputDetail = String(entry.outputItem?.detail || '').trim();
  if (outputDetail) {
    const commandText = extractTaggedSection(outputDetail, 'command');
    const stdoutText = extractTaggedSection(outputDetail, 'stdout');
    const stderrText = extractTaggedSection(outputDetail, 'stderr');
    if (commandText || stdoutText || stderrText) {
      const blocks: string[] = [];
      if (commandText && !command) {
        const preview = buildTextPreview(commandText, 2, 260, '    ');
        if (preview) blocks.push(`command\n${preview}`);
      }
      if (stdoutText) {
        const preview = buildTextPreview(stdoutText, 8, 1400, '    ');
        if (preview) blocks.push(`stdout\n${preview}`);
      }
      if (stderrText) {
        const preview = buildTextPreview(stderrText, 6, 1000, '    ');
        if (preview) blocks.push(`stderr\n${preview}`);
      }
      if (blocks.length > 0) return blocks.join('\n\n');
    }
    return buildTextPreview(outputDetail, 14, 1800, '    ');
  }

  const { resultObject, dataObject } = extractResultPayload(entry.resultItem);
  const firstResult = Array.isArray(dataObject?.results)
    ? (dataObject.results.find((value) => asObject(value)) as UnknownObject | undefined)
    : undefined;
  const stdout = pickString(firstResult?.stdout, dataObject?.stdout, resultObject?.stdout);
  const stderr = pickString(firstResult?.stderr, dataObject?.stderr, resultObject?.stderr);
  if (stdout || stderr) {
    const blocks: string[] = [];
    if (stdout) {
      const preview = buildTextPreview(stdout, 8, 1400, '    ');
      if (preview) blocks.push(`stdout\n${preview}`);
    }
    if (stderr) {
      const preview = buildTextPreview(stderr, 6, 1000, '    ');
      if (preview) blocks.push(`stderr\n${preview}`);
    }
    if (blocks.length > 0) return blocks.join('\n\n');
  }

  if (isApplyPatchTool(entry.toolName)) return '';
  return '';
};

const buildObjectPreview = (
  value: unknown,
  maxLines = 18,
  maxChars = 2200
): string => {
  if (value === null || value === undefined) return '';
  if (typeof value === 'string') {
    return buildTextPreview(value, maxLines, maxChars, '  ');
  }
  try {
    const serialized = JSON.stringify(value, null, 2);
    if (!serialized || serialized === '{}' || serialized === '[]') {
      return '';
    }
    return buildTextPreview(serialized, maxLines, maxChars, '  ');
  } catch {
    return buildTextPreview(String(value), maxLines, maxChars, '  ');
  }
};

const omitObjectKeys = (value: UnknownObject | null, keys: string[]): UnknownObject | null => {
  if (!value) return null;
  const next = { ...value };
  keys.forEach((key) => {
    delete next[key];
  });
  return Object.keys(next).length > 0 ? next : null;
};

// Build a concise view of the model-issued tool call so users can compare it with the result.
const buildGenericModelCallBlock = (entry: RawEntry, command: string): string => {
  const args = extractCallArgs(entry.callItem);
  const blocks: string[] = [];
  if (command) blocks.push(`$ ${command}`);
  const argsPreview = buildObjectPreview(
    omitObjectKeys(args, command ? ['command', 'cmd', 'script', 'raw'] : [])
  );
  if (argsPreview) blocks.push(argsPreview);
  return blocks.join('\n\n').trim();
};

const buildListFilesCallBlock = (entry: RawEntry): string => {
  const args = extractCallArgs(entry.callItem);
  return buildLabeledTextBlock([
    { label: t('chat.toolWorkflow.detail.path'), value: pickString(args?.path) || '.' },
    { label: t('chat.toolWorkflow.detail.depth'), value: toInt(args?.max_depth) || '' }
  ]);
};

const buildSearchContentCallBlock = (entry: RawEntry): string => {
  const args = extractCallArgs(entry.callItem);
  return buildLabeledTextBlock([
    { label: t('chat.toolWorkflow.detail.query'), value: pickString(args?.query) },
    { label: t('chat.toolWorkflow.detail.path'), value: pickString(args?.path) || '.' },
    { label: t('chat.toolWorkflow.detail.pattern'), value: pickString(args?.file_pattern) },
    {
      label: t('chat.toolWorkflow.detail.caseMode'),
      value:
        args?.case_sensitive === true
          ? t('chat.toolWorkflow.detail.caseSensitive')
          : args?.case_sensitive === false
            ? t('chat.toolWorkflow.detail.caseInsensitive')
            : ''
    },
    { label: t('chat.toolWorkflow.detail.depth'), value: toInt(args?.max_depth) || '' },
    { label: t('chat.toolWorkflow.detail.maxFiles'), value: toInt(args?.max_files) || '' },
    {
      label: t('chat.toolWorkflow.detail.context'),
      value: formatContextWindowLabel(args?.context_before, args?.context_after)
    }
  ]);
};

const buildReadFileCallBlock = (entry: RawEntry): string => {
  const args = extractCallArgs(entry.callItem);
  const targets = collectReadTargetLabels(args);
  const summaryBlock = buildLabeledTextBlock([
    { label: t('chat.toolWorkflow.detail.files'), value: targets.length || '' }
  ]);
  const targetBlock = buildBulletListBlock(targets, 6, 160);
  return [summaryBlock, targetBlock].filter(Boolean).join('\n\n');
};

const buildWriteFileCallBlock = (entry: RawEntry, command: string): string => {
  const args = extractCallArgs(entry.callItem);
  const blocks: string[] = [];
  if (command) blocks.push(`$ ${command}`);

  const path = pickString(
    args?.path,
    args?.file,
    args?.filename,
    args?.target,
    args?.target_path,
    args?.targetPath
  );
  const metaBlock = buildLabeledTextBlock([
    { label: t('chat.toolWorkflow.detail.path'), value: path }
  ]);
  if (metaBlock) blocks.push(metaBlock);

  const content = pickString(args?.content, args?.text, args?.input);
  if (content) {
    const contentPreview = buildTerminalStream(content, 'completed', 80, 12000);
    if (contentPreview) blocks.push(contentPreview);
  }

  const extraArgs = omitObjectKeys(args, [
    'command',
    'cmd',
    'script',
    'raw',
    'content',
    'text',
    'input',
    'path',
    'file',
    'filename',
    'target',
    'target_path',
    'targetPath'
  ]);
  const extraPreview = buildObjectPreview(extraArgs);
  if (extraPreview) blocks.push(extraPreview);

  return blocks.join('\n\n').trim();
};

const buildToolResultTextBlock = (entry: RawEntry, command: string, errorText: string): string => {
  const blocks: string[] = [];
  const resultBlock = buildResultBlock(entry);
  const outputBlock = buildOutputBlock(entry, command);
  if (resultBlock) blocks.push(resultBlock);
  if (outputBlock) blocks.push(outputBlock);
  if (errorText) blocks.push(`error: ${errorText}`);
  return blocks.join('\n\n').trim();
};

const buildApplyPatchCallLines = (_command: string, patchDiffBlocks: PatchDiffBlock[]): PatchLine[] => {
  const rows: PatchLine[] = [];
  let cursor = 0;
  const push = (kind: PatchLine['kind'], text: string) => {
    if (!text.trim()) return;
    rows.push({ key: `patch-call-${cursor}`, kind, text });
    cursor += 1;
  };

  patchDiffBlocks.forEach((block) => {
    push('note', block.title);
    block.lines.forEach((line) => {
      if (line.kind === 'add') push('add', line.text);
      else if (line.kind === 'delete') push('delete', line.text);
      else push('meta', line.text);
    });
  });

  return rows;
};

const buildApplyPatchResultLines = (patchEntries: PatchEntry[], errorText: string): PatchLine[] => {
  const rows: PatchLine[] = [];
  let cursor = 0;
  const push = (kind: PatchLine['kind'], text: string) => {
    if (!text.trim()) return;
    rows.push({ key: `patch-result-${cursor}`, kind, text });
    cursor += 1;
  };

  patchEntries.forEach((entry) => {
    const kind: PatchLine['kind'] =
      entry.kind === 'add'
        ? 'add'
        : entry.kind === 'delete'
          ? 'delete'
          : entry.kind === 'move'
            ? 'move'
            : entry.kind === 'other'
              ? 'note'
              : 'update';
    push(kind, `${entry.sign} ${entry.text}`);
  });
  if (errorText) push('error', `error: ${errorText}`);

  return rows;
};

const buildEmptySection = (key: string, title: string, body: string): ToolWorkflowDetailSection => ({
  key,
  title,
  kind: 'text',
  body,
  commandView: null,
  patchLines: [],
  empty: true
});

const buildModelCallSection = (
  entry: RawEntry,
  command: string,
  patchDiffBlocks: PatchDiffBlock[]
): ToolWorkflowDetailSection => {
  const sectionKey = `${entry.key}-model-call`;
  const sectionTitle = t('chat.toolWorkflow.modelCallSection');

  if (isApplyPatchTool(entry.toolName)) {
    const summary = buildApplyPatchCallSummary(entry, patchDiffBlocks);
    const patchFiles = buildApplyPatchCallFiles(patchDiffBlocks);
    if (patchFiles.length > 0) {
      const counts = resolveApplyPatchCounts(entry, patchDiffBlocks);
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'patch',
        summary,
        body: '',
        commandView: null,
        patchLines: [],
        patchView: buildPatchCallView(
          {
            changedFiles: counts.changedFiles,
            hunks: counts.hunks
          },
          patchFiles,
          t
        )
      };
    }
    const patchLines = buildApplyPatchCallLines(command, patchDiffBlocks);
    if (patchLines.length > 0) {
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'patch',
        summary,
        body: '',
        commandView: null,
        patchLines
      };
    }
    const patchInput = buildTextPreview(resolvePatchInput(entry.callItem), 18, 2200, '  ');
    if (patchInput) {
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'text',
        summary,
        body: patchInput,
        commandView: null,
        patchLines: []
      };
    }
  }

  if (isExecuteCommandTool(entry.toolName)) {
    const summary = buildExecuteCommandCallSummary(entry);
    const body = buildExecuteCommandCallBody(entry, command) || buildGenericModelCallBlock(entry, command);
    if (summary || body) {
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'text',
        summary,
        body,
        commandView: null,
        patchLines: []
      };
    }
  }

  const body = isWriteFileTool(entry.toolName)
    ? buildWriteFileCallBlock(entry, command)
    : isReadFileTool(entry.toolName)
      ? buildReadFileCallBlock(entry)
      : isListFilesTool(entry.toolName)
        ? buildListFilesCallBlock(entry)
        : isSearchContentTool(entry.toolName)
          ? buildSearchContentCallBlock(entry)
          : buildGenericModelCallBlock(entry, command);
  if (body) {
    return {
      key: sectionKey,
      title: sectionTitle,
      kind: 'text',
      body,
      commandView: null,
      patchLines: []
    };
  }

  return buildEmptySection(sectionKey, sectionTitle, t('chat.toolWorkflow.modelCallMissing'));
};

const buildToolResultSection = (
  entry: RawEntry,
  command: string,
  status: string,
  errorText: string,
  patchEntries: PatchEntry[],
  compactionDisplay: CompactionDisplay | null
): ToolWorkflowDetailSection => {
  const sectionKey = `${entry.key}-tool-result`;
  const sectionTitle = t('chat.toolWorkflow.toolResultSection');

  if (isApplyPatchTool(entry.toolName)) {
    const summary = buildApplyPatchResultSummary(entry);
    const counts = resolveApplyPatchCounts(entry);
    const patchFiles = buildApplyPatchResultFiles(patchEntries, errorText);
    if (patchFiles.length > 0) {
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'patch',
        summary,
        body: '',
        commandView: null,
        patchLines: [],
        patchView: buildPatchResultView(counts, patchFiles, t)
      };
    }
    const patchLines = buildApplyPatchResultLines(patchEntries, errorText);
    if (patchLines.length > 0) {
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'patch',
        summary,
        body: '',
        commandView: null,
        patchLines
      };
    }
    if (summary) {
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'text',
        summary,
        body: errorText || '',
        commandView: null,
        patchLines: [],
        empty: !errorText
      };
    }
  }

  if (isExecuteCommandTool(entry.toolName)) {
    if (entry.outputItem || entry.resultItem || errorText) {
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'command',
        body: '',
        commandView: {
          // Keep command workflows concise: show command + terminal output in one block.
          ...buildExecuteCommandView(entry, command, status, errorText, true),
          showExitCode: false
        },
        patchLines: []
      };
    }
  } else if (isCompactionTool(entry.toolName) && compactionDisplay) {
    const body = [errorText ? `error: ${errorText}` : '']
      .filter((item) => item.trim())
      .join('\n');
    return {
      key: sectionKey,
      title: sectionTitle,
      kind: 'compaction',
      summary: compactionDisplay.resultSummary,
      body,
      commandView: null,
      patchLines: [],
      compactionView: compactionDisplay.view
    };
  } else {
    const { resultObject, dataObject } = extractResultPayload(entry.resultItem);
    const structuredView = buildStructuredToolResultView(entry.toolName, resultObject, dataObject, t);
    if (structuredView) {
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'structured',
        body: errorText ? `error: ${errorText}` : '',
        commandView: null,
        patchLines: [],
        structuredView
      };
    }
    const body = buildToolResultTextBlock(entry, command, errorText);
    if (body) {
      return {
        key: sectionKey,
        title: sectionTitle,
        kind: 'text',
        body,
        commandView: null,
        patchLines: []
      };
    }
  }

  const placeholder =
    status === 'loading' || status === 'pending'
      ? t('chat.toolWorkflow.toolResultPending')
      : t('chat.toolWorkflow.toolResultMissing');
  return buildEmptySection(sectionKey, sectionTitle, placeholder);
};

const buildErrorText = (resultItem: WorkflowItem | null): string => {
  if (!resultItem) return '';
  const detailObject = parseDetailObject(resultItem.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);
  const error = pickString(resultObject?.error, dataObject?.error);
  const code = pickString(dataObject?.error_code);
  if (error && code) return `${error} (${code})`;
  if (error) return error;
  return code;
};

const resolveEntryStatus = (entry: RawEntry): string =>
  normalizeStatus(entry.resultItem?.status || entry.outputItem?.status || entry.callItem?.status || 'completed');

const resolveCompactionDetailObject = (entry: RawEntry): UnknownObject | null =>
  parseDetailObject(entry.resultItem?.detail)
  || parseDetailObject(entry.outputItem?.detail)
  || parseDetailObject(entry.callItem?.detail);

const buildEntryView = (entry: RawEntry): ToolEntryView => {
  const rawCommand = pickString(
    resolveCommandFromCall(entry.callItem),
    resolveCommandFromOutput(entry.outputItem),
    resolveCommandFromResult(entry.resultItem)
  );
  const command = isExecuteCommandTool(entry.toolName) ? rawCommand : '';
  const toolDisplay = resolveSummaryToolDisplay(
    entry.toolName,
    entry.toolName || t('chat.workflow.toolUnknown')
  );
  const patchEntries = buildApplyPatchEntries(entry.resultItem, entry.toolName);
  const patchDiffBlocks = buildApplyPatchDiffBlocks(entry.callItem, entry.toolName);
  const pathHints = collectEntryPathHints(entry, patchEntries, patchDiffBlocks);
  const status = resolveEntryStatus(entry);
  const compactionDisplay = isCompactionTool(entry.toolName)
    ? buildCompactionDisplay(resolveCompactionDetailObject(entry), status, t)
    : null;
  const errorText = status === 'failed' ? buildErrorText(entry.resultItem) : '';
  const summaryTitle = compactionDisplay?.summaryTitle || composeEntryTitle(entry, toolDisplay, command, pathHints);
  const durationLabel = formatDurationLabel(extractDurationMs(entry));
  const shouldKeepModelCall = isWriteFileTool(entry.toolName) || isApplyPatchTool(entry.toolName);
  const modelCallSection = buildModelCallSection(entry, command, patchDiffBlocks);
  const toolResultSection = buildToolResultSection(
    entry,
    command,
    status,
    errorText,
    patchEntries,
    compactionDisplay
  );
  const sections = shouldKeepModelCall ? [modelCallSection, toolResultSection] : [toolResultSection];

  return {
    key: entry.key,
    summaryTitle,
    summaryNote: compactionDisplay?.summaryNote || '',
    summaryNoteTone: compactionDisplay?.summaryNoteTone || '',
    isCompaction: Boolean(compactionDisplay),
    compactionView: compactionDisplay?.view || null,
    status,
    statusLabel: statusLabel(status),
    durationLabel,
    sections
  };
};

const findLastPendingIndex = (rows: RawEntry[]): number => {
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    if (!rows[index].resultItem) return index;
  }
  return -1;
};

const normalizeWorkflowRef = (value: unknown): string => String(value || '').trim();

const dedupeAdjacentToolItems = (items: WorkflowItem[]): WorkflowItem[] => {
  const output: WorkflowItem[] = [];
  let lastKey = '';
  items.forEach((item) => {
    const kind = resolveToolEventKind(item);
    if (!kind) {
      output.push(item);
      lastKey = '';
      return;
    }
    const key = [
      kind,
      resolveToolName(item).trim().toLowerCase(),
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

const buildEntries = (): ToolEntryView[] => {
  return buildWorkflowToolRuns(props.items)
    .map(buildEntryView)
    .filter((entry) => !entry.isCompaction);
};

const entries = computed<ToolEntryView[]>(() => {
  if (!chatPerf.enabled()) {
    return buildEntries();
  }
  return chatPerf.time(
    'chat_workflow_entries_build',
    () => buildEntries(),
    {
      itemCount: Array.isArray(props.items) ? props.items.length : 0
    }
  );
});

watch(
  entries,
  (nextEntries) => {
    const validKeys = new Set(nextEntries.map((entry) => entry.key));
    pruneStreamTracking(validKeys);
    const nextExpanded = new Set<string>();
    expandedKeys.value.forEach((key) => {
      if (validKeys.has(key)) nextExpanded.add(key);
    });
    expandedKeys.value = nextExpanded;
    void nextTick(() => {
      syncStreamAutoStick();
      if (shouldAutoScrollWorkflow()) {
        scrollWorkflowToBottom();
      }
      scheduleWorkflowLayoutChange();
    });
  },
  { immediate: true }
);

const handleEntryToggle = (key: string, event: Event) => {
  const target = event.target as HTMLDetailsElement | null;
  if (!target) return;
  const next = new Set(expandedKeys.value);
  if (target.open) next.add(key);
  else next.delete(key);
  expandedKeys.value = next;
  void nextTick(() => {
    if (shouldAutoScrollWorkflow()) {
      scrollWorkflowToBottom();
    }
    scheduleWorkflowLayoutChange();
  });
};

const latestEntry = computed(() => (entries.value.length > 0 ? entries.value[entries.value.length - 1] : null));
// Do not expose workflow shell during pure model streaming; show it only after the first tool run appears.
const shouldRender = computed(() => props.visible && entries.value.length > 0);

onBeforeUnmount(() => {
  if (typeof window !== 'undefined' && workflowLayoutFrame !== null) {
    window.cancelAnimationFrame(workflowLayoutFrame);
    workflowLayoutFrame = null;
  }
});
</script>

<style scoped>
.message-tool-workflow-shell {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-top: 6px;
}

.tool-workflow-banner-fade-enter-active,
.tool-workflow-banner-fade-leave-active {
  transition: opacity 0.22s ease, transform 0.22s ease;
}

.tool-workflow-banner-fade-enter-from,
.tool-workflow-banner-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

.message-tool-workflow {
  --workflow-term-bg: #0f1622;
  --workflow-term-bg-soft: #141e2e;
  --workflow-term-bg-hover: #1a2739;
  --workflow-term-border: #263348;
  --workflow-term-border-strong: #32445f;
  --workflow-term-text: #e5edf8;
  --workflow-term-muted: #93a5c0;
  --workflow-term-code: #f1f5ff;
  --workflow-term-scroll-track: #0d1420;
  --workflow-term-scroll-thumb: #3b4b63;
  --workflow-banner-text: var(--chat-text, #0f172a);
  --workflow-banner-muted: var(--chat-muted, #64748b);
  --workflow-banner-panel: var(--chat-panel, rgba(255, 255, 255, 0.1));
  border: none;
  background: transparent;
  padding: 0;
  color: var(--workflow-term-text);
}

.tool-workflow-banner {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  width: 100%;
  padding: 10px 12px;
  border-radius: 12px;
  border: 1px solid rgba(var(--chat-primary-rgb, 59, 130, 246), 0.3);
  background: linear-gradient(
    180deg,
    rgba(var(--chat-primary-rgb, 59, 130, 246), 0.14),
    var(--workflow-banner-panel)
  );
  color: var(--workflow-banner-text);
  text-align: left;
  cursor: pointer;
  transition: border-color 0.18s ease, transform 0.18s ease, box-shadow 0.18s ease;
}

.tool-workflow-banner:hover {
  border-color: rgba(var(--chat-primary-rgb, 59, 130, 246), 0.42);
  box-shadow: 0 6px 16px rgba(15, 23, 42, 0.08);
  transform: translateY(-1px);
}

.tool-workflow-banner-dot {
  width: 9px;
  height: 9px;
  border-radius: 999px;
  flex: 0 0 auto;
  background: rgba(59, 130, 246, 0.98);
  box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.16);
}

.tool-workflow-banner.is-loading .tool-workflow-banner-dot,
.tool-workflow-banner.is-pending .tool-workflow-banner-dot {
  animation: tool-workflow-banner-live-pulse 1.2s ease-in-out infinite;
}

.tool-workflow-banner.is-completed .tool-workflow-banner-dot {
  background: rgba(34, 197, 94, 0.96);
  box-shadow: 0 0 0 3px rgba(34, 197, 94, 0.14);
}

.tool-workflow-banner.is-completed.is-animated {
  animation: tool-workflow-banner-complete 0.38s ease-out;
}

.tool-workflow-banner.is-failed .tool-workflow-banner-dot {
  background: rgba(248, 113, 113, 0.96);
  box-shadow: 0 0 0 3px rgba(248, 113, 113, 0.14);
}

.tool-workflow-banner.is-completed.tone-info {
  border-color: rgba(34, 197, 94, 0.3);
  background: linear-gradient(180deg, rgba(22, 163, 74, 0.12), var(--workflow-banner-panel));
}

.tool-workflow-banner.tone-warning {
  border-color: rgba(245, 158, 11, 0.26);
  background: linear-gradient(180deg, rgba(217, 119, 6, 0.14), var(--workflow-banner-panel));
}

.tool-workflow-banner.tone-warning .tool-workflow-banner-dot {
  background: rgba(245, 158, 11, 0.96);
  box-shadow: 0 0 0 3px rgba(245, 158, 11, 0.14);
}

.tool-workflow-banner-copy {
  min-width: 0;
  flex: 1 1 auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.tool-workflow-banner-main {
  min-width: 0;
  flex: 1 1 auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-banner-row,
.tool-workflow-banner-usage-head,
.tool-workflow-banner-usage-legend {
  display: flex;
  align-items: center;
  gap: 10px;
}

.tool-workflow-banner-row {
  align-items: flex-start;
}

.tool-workflow-banner-title {
  color: var(--workflow-banner-text);
  font-size: 12px;
  font-weight: 700;
  line-height: 1.35;
}

.tool-workflow-banner-description {
  color: var(--workflow-banner-muted);
  font-size: 11px;
  line-height: 1.45;
  white-space: pre-wrap;
  word-break: break-word;
}

.tool-workflow-banner-stage {
  flex: 0 0 auto;
  border-radius: 999px;
  padding: 2px 8px;
  border: 1px solid rgba(148, 163, 184, 0.3);
  background: rgba(255, 255, 255, 0.56);
  color: var(--workflow-banner-muted);
  font-size: 10px;
  font-weight: 700;
}

.tool-workflow-banner-note {
  flex: 0 0 auto;
  border-radius: 999px;
  padding: 2px 8px;
  background: rgba(var(--chat-primary-rgb, 59, 130, 246), 0.14);
  color: var(--workflow-banner-text);
  border: 1px solid rgba(var(--chat-primary-rgb, 59, 130, 246), 0.3);
  font-size: 10px;
  font-weight: 700;
}

.tool-workflow-banner-note.is-info {
  background: rgba(var(--chat-primary-rgb, 59, 130, 246), 0.14);
  color: var(--workflow-banner-text);
  border-color: rgba(var(--chat-primary-rgb, 59, 130, 246), 0.34);
}

.tool-workflow-banner-note.is-success {
  background: rgba(22, 163, 74, 0.18);
  border-color: rgba(134, 239, 172, 0.3);
  color: var(--workflow-banner-text);
}

.tool-workflow-banner-note.is-warning {
  background: rgba(217, 119, 6, 0.18);
  border-color: rgba(252, 211, 77, 0.3);
  color: var(--workflow-banner-text);
}

.tool-workflow-banner-usage {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid rgba(148, 163, 184, 0.24);
  background: rgba(255, 255, 255, 0.56);
}

.tool-workflow-banner-usage.is-success {
  border-color: rgba(134, 239, 172, 0.26);
  background: rgba(22, 163, 74, 0.08);
}

.tool-workflow-banner-usage.is-warning {
  border-color: rgba(252, 211, 77, 0.26);
  background: rgba(217, 119, 6, 0.08);
}

.tool-workflow-banner-usage.is-danger {
  border-color: rgba(248, 113, 113, 0.26);
  background: rgba(127, 29, 29, 0.18);
}

.tool-workflow-banner-usage-limit,
.tool-workflow-banner-usage-hint,
.tool-workflow-banner-usage-label {
  font-size: 10px;
  line-height: 1.4;
}

.tool-workflow-banner-usage-limit {
  color: var(--workflow-banner-muted);
  font-weight: 700;
}

.tool-workflow-banner-usage-hint {
  color: var(--workflow-banner-text);
  margin-left: auto;
}

.tool-workflow-banner-usage-track {
  position: relative;
  height: 10px;
  border-radius: 999px;
  overflow: hidden;
  background: rgba(148, 163, 184, 0.14);
}

.tool-workflow-banner-usage-fill {
  position: absolute;
  top: 0;
  left: 0;
  bottom: 0;
  border-radius: 999px;
}

.tool-workflow-banner-usage-fill.is-before {
  background: rgba(248, 113, 113, 0.5);
}

.tool-workflow-banner-usage-fill.is-after {
  background: rgba(59, 130, 246, 0.8);
}

.tool-workflow-banner-usage-label {
  color: var(--workflow-banner-muted);
}

.tool-workflow-banner-usage-label.is-before {
  color: #fecaca;
}

.tool-workflow-banner-usage-label.is-after {
  color: #bfdbfe;
  margin-left: auto;
  text-align: right;
}

.tool-workflow-banner-failure {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px solid rgba(248, 113, 113, 0.26);
  background: rgba(254, 226, 226, 0.68);
}

.tool-workflow-banner-failure-title {
  color: #991b1b;
  font-size: 11px;
  font-weight: 700;
}

.tool-workflow-banner-failure-description {
  color: #b91c1c;
  font-size: 11px;
  line-height: 1.45;
}

.tool-workflow-banner-failure-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.tool-workflow-banner-failure-chip {
  border-radius: 999px;
  padding: 2px 8px;
  border: 1px solid rgba(239, 68, 68, 0.22);
  background: rgba(255, 255, 255, 0.72);
  color: #991b1b;
  font-size: 10px;
  font-weight: 700;
}

.message-tool-workflow > summary {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--chat-text);
  cursor: pointer;
  font-weight: 600;
  list-style: none;
  font-size: 12px;
}

.message-tool-workflow > summary::marker {
  display: none;
}

.message-tool-workflow > summary::before {
  content: '▸';
  display: inline-block;
  transition: color 0.2s ease;
  color: var(--chat-text);
  opacity: 0.85;
}

.message-tool-workflow[open] > summary::before {
  content: '▾';
}

.tool-workflow-title {
  color: var(--chat-text);
}

.tool-workflow-spacer {
  flex: 1 1 auto;
}

.tool-workflow-latest {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--chat-muted);
  font-size: 12px;
  font-weight: 500;
  opacity: 0.95;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tool-workflow-list {
  margin-top: 6px;
  height: 320px;
  max-height: 320px;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
  scrollbar-gutter: stable;
  padding: 10px 10px 14px;
  scroll-padding-bottom: 14px;
  border-radius: 12px;
  border: 1px solid var(--workflow-term-border);
  background: var(--workflow-term-bg);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.02);
  scrollbar-color: var(--workflow-term-scroll-thumb) var(--workflow-term-scroll-track);
}

.tool-workflow-list::-webkit-scrollbar {
  width: 8px;
}

.tool-workflow-list::-webkit-scrollbar-track {
  background: var(--workflow-term-scroll-track);
}

.tool-workflow-list::-webkit-scrollbar-thumb {
  background: var(--workflow-term-scroll-thumb);
  border-radius: 999px;
}

.tool-workflow-empty {
  color: var(--workflow-term-muted);
  font-size: 12px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px dashed var(--workflow-term-border-strong);
  background: var(--workflow-term-bg-soft);
}

.tool-workflow-entry {
  border: 1px solid transparent;
  border-radius: 10px;
  background: transparent;
  overflow: hidden;
  transition: border-color 0.16s ease, background 0.16s ease;
}

.tool-workflow-entry + .tool-workflow-entry {
  margin-top: 4px;
}

.tool-workflow-entry:hover,
.tool-workflow-entry[open] {
  border-color: var(--workflow-term-border-strong);
  background: var(--workflow-term-bg-hover);
}

.tool-workflow-entry > summary {
  list-style: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  cursor: pointer;
  color: var(--workflow-term-text);
}

.tool-workflow-entry > summary::marker {
  display: none;
}

.tool-workflow-entry > summary::before {
  content: '▸';
  font-size: 10px;
  color: var(--workflow-term-muted);
  transition: color 0.18s ease;
}

.tool-workflow-entry[open] > summary::before {
  content: '▾';
}

.tool-workflow-entry-title {
  min-width: 0;
  flex: 1 1 auto;
  font-size: 12px;
  font-weight: 600;
  color: var(--workflow-term-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tool-workflow-entry-lamp {
  width: 7px;
  height: 7px;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.78);
  box-shadow: 0 0 0 2px rgba(148, 163, 184, 0.12);
  flex: 0 0 auto;
}

.tool-workflow-entry-lamp.is-loading,
.tool-workflow-entry-lamp.is-pending {
  background: rgba(59, 130, 246, 0.98);
  box-shadow: 0 0 0 2px rgba(59, 130, 246, 0.18);
  animation: tool-workflow-status-pulse 1.3s ease-in-out infinite;
}

.tool-workflow-entry-lamp.is-completed {
  background: rgba(34, 197, 94, 0.95);
  box-shadow: 0 0 0 2px rgba(34, 197, 94, 0.16);
}

.tool-workflow-entry-lamp.is-failed {
  background: rgba(248, 113, 113, 0.98);
  box-shadow: 0 0 0 2px rgba(239, 68, 68, 0.16);
}

.tool-workflow-entry-duration {
  flex: 0 0 auto;
  color: var(--workflow-term-muted);
  font-size: 11px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-entry-status {
  flex: 0 0 auto;
  border-radius: 999px;
  padding: 1px 7px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.2px;
}

.tool-workflow-entry-status.is-loading,
.tool-workflow-entry-status.is-pending {
  background: rgba(59, 130, 246, 0.26);
  border: 1px solid rgba(147, 197, 253, 0.5);
  color: #dbeafe;
}

.tool-workflow-entry-status.is-completed {
  background: rgba(22, 163, 74, 0.24);
  border: 1px solid rgba(134, 239, 172, 0.48);
  color: #dcfce7;
}

.tool-workflow-entry-status.is-failed {
  background: rgba(220, 38, 38, 0.24);
  border: 1px solid rgba(254, 202, 202, 0.45);
  color: #fee2e2;
}

@keyframes tool-workflow-status-pulse {
  0%,
  100% {
    opacity: 1;
    transform: scale(1);
  }
  50% {
    opacity: 0.55;
    transform: scale(0.78);
  }
}

@keyframes tool-workflow-banner-live-pulse {
  0%,
  100% {
    transform: scale(1);
    opacity: 1;
  }
  50% {
    transform: scale(0.82);
    opacity: 0.6;
  }
}

@keyframes tool-workflow-banner-complete {
  0% {
    transform: translateY(3px);
    opacity: 0.72;
  }
  100% {
    transform: translateY(0);
    opacity: 1;
  }
}

.tool-workflow-entry-body {
  padding: 0 10px 10px 24px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

</style>
