<template>
  <details
    v-if="shouldRender"
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
          <span class="tool-workflow-entry-title">{{ entry.summaryTitle }}</span>
          <span v-if="entry.durationLabel" class="tool-workflow-entry-duration">{{ entry.durationLabel }}</span>
          <span :class="['tool-workflow-entry-status', `is-${entry.status}`]">{{ entry.statusLabel }}</span>
        </summary>

        <div class="tool-workflow-entry-body">
          <div
            v-if="entry.viewKind === 'command' && entry.commandView"
            class="tool-workflow-main tool-workflow-main--command"
          >
            <div class="tool-workflow-terminal-head">{{ entry.commandView.shell }}</div>

            <pre
              class="tool-workflow-terminal-body"
              :ref="(el) => bindStreamBodyRef(entry.key, 'stdout', el)"
              @scroll="handleStreamBodyScroll(entry.key, 'stdout', $event)"
            >{{ entry.commandView.terminalText }}</pre>

            <div class="tool-workflow-terminal-footer">
              <span
                v-if="entry.commandView.exitCode !== null"
                class="tool-workflow-terminal-exit-code"
              >
                exit {{ entry.commandView.exitCode }}
              </span>
            </div>
          </div>

          <div v-else-if="entry.viewKind === 'patch'" class="tool-workflow-main tool-workflow-main--patch">
            <div
              v-for="line in entry.patchLines"
              :key="line.key"
              :class="['tool-workflow-patch-line', `is-${line.kind}`]"
            >
              {{ line.text }}
            </div>
          </div>

          <pre v-else-if="entry.mainBlock" class="tool-workflow-main">{{ entry.mainBlock }}</pre>
        </div>
      </details>
    </div>
  </details>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch, type ComponentPublicInstance } from 'vue';

import { useI18n } from '@/i18n';
import { buildToolResultPreview } from './toolWorkflowPreview';
import { chatPerf } from '@/utils/chatPerf';

type WorkflowItem = {
  id?: string | number;
  title?: string;
  detail?: string;
  status?: string;
  isTool?: boolean;
  eventType?: string;
  toolName?: string;
  toolCallId?: string | number;
};

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
  status: string;
  statusLabel: string;
  durationLabel: string;
  viewKind: 'text' | 'command' | 'patch';
  mainBlock: string;
  commandView: CommandView | null;
  patchLines: PatchLine[];
};

type CommandView = {
  command: string;
  shell: string;
  terminalText: string;
  exitCode: number | null;
};

type CommandRecord = {
  command: string;
  stdout: string;
  stderr: string;
  returncode: number | null;
};

type PatchLine = {
  key: string;
  kind: 'meta' | 'note' | 'add' | 'delete' | 'move' | 'update' | 'error';
  text: string;
};

type RawEntry = {
  key: string;
  toolName: string;
  callItem: WorkflowItem | null;
  outputItem: WorkflowItem | null;
  resultItem: WorkflowItem | null;
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

const { t } = useI18n();
const expandedKeys = ref<Set<string>>(new Set());
const streamBodyRefMap = new Map<string, HTMLPreElement>();
const streamFollowState = new Map<string, boolean>();
const workflowRef = ref<HTMLDetailsElement | null>(null);
const workflowListRef = ref<HTMLElement | null>(null);
const workflowFollow = ref(true);
const detailParseCache = new Map<string, UnknownObject | false>();
const previewCache = new Map<string, string>();

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
  if (status === 'loading' || status === 'pending') return t('chat.toolWorkflow.statusRunning');
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
  if (eventType === 'tool_result') return 'result';

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
      text: path && toPath && path !== toPath ? `${path} -> ${toPath}` : path || toPath
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
      title: pathText || `file-${index + 1}`,
      pathHint: pathText,
      lines
    };
  });
};

const buildApplyPatchResultNote = (resultItem: WorkflowItem | null, toolName: string): string => {
  if (!resultItem || !isApplyPatchTool(toolName)) return '';
  const detailObject = parseDetailObject(resultItem.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);

  const changedFiles = toInt(dataObject?.changed_files, resultObject?.changed_files);
  const hunksApplied = toInt(dataObject?.hunks_applied, resultObject?.hunks_applied);
  const added = toInt(dataObject?.added, resultObject?.added);
  const updated = toInt(dataObject?.updated, resultObject?.updated);
  const deleted = toInt(dataObject?.deleted, resultObject?.deleted);
  const moved = toInt(dataObject?.moved, resultObject?.moved);

  const parts: string[] = [];
  if (changedFiles > 0) parts.push(`files ${changedFiles}`);
  if (hunksApplied > 0) parts.push(`hunks ${hunksApplied}`);
  if (added + updated + deleted + moved > 0) {
    parts.push(`+${added} ~${updated} -${deleted} >${moved}`);
  }
  return parts.join(' · ');
};

const buildApplyPatchLines = (
  command: string,
  resultNote: string,
  patchEntries: PatchEntry[],
  patchDiffBlocks: PatchDiffBlock[],
  errorText: string
): PatchLine[] => {
  const rows: PatchLine[] = [];
  let cursor = 0;
  const push = (kind: PatchLine['kind'], text: string) => {
    if (!text.trim()) return;
    rows.push({ key: `patch-${cursor}`, kind, text });
    cursor += 1;
  };

  if (command) push('meta', `$ ${command}`);
  if (resultNote) push('note', resultNote);
  patchEntries.forEach((entry) => {
    const kind: PatchLine['kind'] =
      entry.kind === 'add'
        ? 'add'
        : entry.kind === 'delete'
          ? 'delete'
          : entry.kind === 'move'
            ? 'move'
            : 'update';
    push(kind, `${entry.sign} ${entry.text}`);
  });
  patchDiffBlocks.forEach((block) => {
    push('meta', `@@ ${block.title}`);
    block.lines.forEach((line) => {
      if (line.kind === 'add') push('add', line.text);
      else if (line.kind === 'delete') push('delete', line.text);
      else push('meta', line.text);
    });
  });
  if (errorText) push('error', `error: ${errorText}`);
  return rows;
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

const composeSummaryTitle = (base: string, pathHints: string[]): string => {
  if (!pathHints.length) return truncateSingleLine(base);
  const visible = pathHints.slice(0, FILE_HINT_SUMMARY_LIMIT);
  const moreCount = Math.max(pathHints.length - visible.length, 0);
  const suffix = moreCount > 0 ? ` +${moreCount}` : '';
  return truncateSingleLine(`${base} ${visible.join(', ')}${suffix}`);
};

const composeEntryTitle = (toolDisplay: string, command: string, pathHints: string[]): string => {
  if (command) {
    return truncateSingleLine(`${toolDisplay} ${command}`);
  }
  return composeSummaryTitle(toolDisplay, pathHints);
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
  if (!sections.length) return buildTextPreview(content, 14, 1800, '');

  const fileBlocks = sections.slice(0, 3).map((section) => {
    const path = section.path || '(unknown)';
    const preview = buildTextPreview(section.body, 12, 1400, '');
    return `${path}\n${preview}`;
  });
  if (sections.length > fileBlocks.length) {
    fileBlocks.push(`... (+${sections.length - fileBlocks.length} files)`);
  }
  return fileBlocks.join('\n\n');
};

const buildListFilesResultBlock = (dataObject: UnknownObject | null): string => {
  const items = Array.isArray(dataObject?.items) ? (dataObject.items as unknown[]) : [];
  if (!items.length) return '';
  const normalized = items.map((item) => String(item || '').trim()).filter(Boolean);
  if (!normalized.length) return '';
  const visible = normalized.slice(0, 30);
  const rows = visible.map((item) => `- ${item}`);
  if (normalized.length > visible.length) {
    rows.push(`... (+${normalized.length - visible.length})`);
  }
  return rows.join('\n');
};

const buildSearchResultBlock = (dataObject: UnknownObject | null): string => {
  const matches = Array.isArray(dataObject?.matches) ? (dataObject.matches as unknown[]) : [];
  if (!matches.length) return '';
  const normalized = matches.map((item) => String(item || '').trim()).filter(Boolean);
  if (!normalized.length) return '';
  const visible = normalized.slice(0, 24);
  const rows = visible.map((item) => `- ${truncateSingleLine(item, 160)}`);
  if (normalized.length > visible.length) {
    rows.push(`... (+${normalized.length - visible.length})`);
  }
  return rows.join('\n');
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

  const rows: string[] = [];
  if (path) rows.push(path);
  if (bytes > 0) rows.push(`${bytes} bytes`);
  return rows.join('\n');
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
  status: string
): string => {
  const lines: string[] = [];
  lines.push(`$ ${command || '(command)'}`);

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
  errorText: string
): CommandView => {
  const { resultObject, dataObject } = extractResultPayload(entry.resultItem);
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

  return {
    command: commandText,
    shell: 'bash',
    terminalText: buildExecuteCommandTerminalText(
      commandText,
      displayStdout,
      displayStderr,
      previewRaw,
      errorText,
      status
    ),
    exitCode
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

const buildMainBlock = (
  command: string,
  resultBlock: string,
  outputBlock: string,
  errorText: string
): string => {
  const rows: string[] = [];
  if (command) rows.push(`$ ${command}`);
  if (resultBlock) {
    if (rows.length > 0) rows.push('');
    rows.push(resultBlock);
  }
  if (outputBlock) {
    if (rows.length > 0) rows.push('');
    rows.push(outputBlock);
  }
  if (errorText) {
    if (rows.length > 0) rows.push('');
    rows.push(`error: ${errorText}`);
  }
  return rows.join('\n').trim();
};

const buildWriteFileMainBlock = (entry: RawEntry): string => {
  const { resultObject, dataObject } = extractResultPayload(entry.resultItem);
  const metaBlock = buildWriteFileResultBlock(resultObject, dataObject);
  const args = extractCallArgs(entry.callItem);
  const content = pickString(args?.content, args?.text, args?.input);
  if (!content) return metaBlock;

  const contentPreview = buildTerminalStream(content, 'completed', 80, 12000);
  if (!contentPreview) return metaBlock;
  if (!metaBlock) return contentPreview;
  return `${metaBlock}\n\n${contentPreview}`;
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

const buildEntryView = (entry: RawEntry): ToolEntryView => {
  const rawCommand = pickString(
    resolveCommandFromCall(entry.callItem),
    resolveCommandFromOutput(entry.outputItem),
    resolveCommandFromResult(entry.resultItem)
  );
  const command = isExecuteCommandTool(entry.toolName) ? rawCommand : '';
  const toolDisplay = entry.toolName || t('chat.workflow.toolUnknown');
  const patchEntries = buildApplyPatchEntries(entry.resultItem, entry.toolName);
  const patchDiffBlocks = buildApplyPatchDiffBlocks(entry.callItem, entry.toolName);
  const pathHints = collectEntryPathHints(entry, patchEntries, patchDiffBlocks);
  const summaryTitle = composeEntryTitle(toolDisplay, command, pathHints);
  const status = resolveEntryStatus(entry);
  const errorText = status === 'failed' ? buildErrorText(entry.resultItem) : '';
  const durationLabel = formatDurationLabel(extractDurationMs(entry));

  if (isApplyPatchTool(entry.toolName)) {
    const resultNote = buildApplyPatchResultNote(entry.resultItem, entry.toolName);
    return {
      key: entry.key,
      summaryTitle,
      status,
      statusLabel: statusLabel(status),
      durationLabel,
      viewKind: 'patch',
      mainBlock: '',
      commandView: null,
      patchLines: buildApplyPatchLines(command, resultNote, patchEntries, patchDiffBlocks, errorText)
    };
  }

  if (isExecuteCommandTool(entry.toolName)) {
    return {
      key: entry.key,
      summaryTitle,
      status,
      statusLabel: statusLabel(status),
      durationLabel,
      viewKind: 'command',
      mainBlock: '',
      commandView: buildExecuteCommandView(entry, command, status, errorText),
      patchLines: []
    };
  }

  if (isWriteFileTool(entry.toolName)) {
    return {
      key: entry.key,
      summaryTitle,
      status,
      statusLabel: statusLabel(status),
      durationLabel,
      viewKind: 'text',
      mainBlock: buildWriteFileMainBlock(entry),
      commandView: null,
      patchLines: []
    };
  }

  const outputBlock = buildOutputBlock(entry, command);
  const resultBlock = buildResultBlock(entry);
  const mainBlock = buildMainBlock(command, resultBlock, outputBlock, errorText);

  return {
    key: entry.key,
    summaryTitle,
    status,
    statusLabel: statusLabel(status),
    durationLabel,
    viewKind: 'text',
    mainBlock,
    commandView: null,
    patchLines: []
  };
};

const findLastPendingIndex = (rows: RawEntry[]): number => {
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    if (!rows[index].resultItem) return index;
  }
  return -1;
};

const normalizeWorkflowRef = (value: unknown): string => String(value || '').trim();

const buildEntries = (): ToolEntryView[] => {
  const rows: RawEntry[] = [];
  const pendingByTool = new Map<string, number[]>();
  const rowIndexByCallId = new Map<string, number>();

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

  props.items.forEach((item, index) => {
    const kind = resolveToolEventKind(item);
    if (!kind) return;

    const toolName = resolveToolName(item);
    const toolKey = toolName.trim().toLowerCase() || '__unknown__';
    const itemId = normalizeWorkflowRef(item.id) || `tool-entry-${index}`;
    const toolCallId = normalizeWorkflowRef(item.toolCallId);

    if (kind === 'call') {
      const existingIndex = rowIndexByCallId.get(itemId);
      if (typeof existingIndex === 'number') {
        rows[existingIndex].callItem = item;
        if (!rows[existingIndex].toolName && toolName) rows[existingIndex].toolName = toolName;
        rowIndexByCallId.set(itemId, existingIndex);
        if (!rows[existingIndex].resultItem) {
          enqueuePending(toolKey, existingIndex);
        }
      } else {
        rows.push({ key: itemId, toolName, callItem: item, outputItem: null, resultItem: null });
        const rowIndex = rows.length - 1;
        rowIndexByCallId.set(itemId, rowIndex);
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

  return rows.map(buildEntryView);
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
  (nextEntries, prevEntries = []) => {
    const validKeys = new Set(nextEntries.map((entry) => entry.key));
    pruneStreamTracking(validKeys);
    const previousKeys = new Set(prevEntries.map((entry) => entry.key));
    const newestEntry = [...nextEntries].reverse().find((entry) => !previousKeys.has(entry.key));
    const nextExpanded = new Set<string>();
    if (newestEntry) {
      nextExpanded.add(newestEntry.key);
    } else {
      expandedKeys.value.forEach((key) => {
        if (validKeys.has(key)) nextExpanded.add(key);
      });
      for (let index = nextEntries.length - 1; index >= 0; index -= 1) {
        const entry = nextEntries[index];
        if (entry.status === 'loading' || entry.status === 'pending') {
          nextExpanded.add(entry.key);
          break;
        }
      }
    }
    expandedKeys.value = nextExpanded;
    void nextTick(() => {
      syncStreamAutoStick();
      if (shouldAutoScrollWorkflow()) {
        scrollWorkflowToBottom();
      }
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
};

const latestEntry = computed(() => (entries.value.length > 0 ? entries.value[entries.value.length - 1] : null));
const shouldRender = computed(() => props.visible && (props.loading || entries.value.length > 0));
</script>

<style scoped>
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
  border: none;
  background: transparent;
  padding: 6px 0 0;
  color: var(--workflow-term-text);
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
  clip-path: inset(0 round 12px);
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

.tool-workflow-entry-body {
  padding: 0 10px 10px 24px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-main {
  margin: 0;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid var(--workflow-term-border);
  background: var(--workflow-term-bg-soft);
  color: var(--workflow-term-text);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 320px;
  overflow: auto;
  scrollbar-color: var(--workflow-term-scroll-thumb) var(--workflow-term-scroll-track);
  clip-path: inset(0 round 10px);
}

.tool-workflow-main::-webkit-scrollbar,
.tool-workflow-terminal-body::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}

.tool-workflow-main::-webkit-scrollbar-track,
.tool-workflow-terminal-body::-webkit-scrollbar-track {
  background: var(--workflow-term-scroll-track);
}

.tool-workflow-main::-webkit-scrollbar-thumb,
.tool-workflow-terminal-body::-webkit-scrollbar-thumb {
  background: var(--workflow-term-scroll-thumb);
  border-radius: 999px;
}

.tool-workflow-main--command {
  white-space: normal;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-terminal-head {
  color: var(--workflow-term-muted);
  font-size: 11px;
  letter-spacing: 0.2px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-terminal-body {
  margin: 0;
  white-space: pre-wrap;
  word-break: break-word;
  font-size: 12px;
  line-height: 1.5;
  color: var(--workflow-term-code);
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
  max-height: 260px;
  overflow: auto;
  padding: 0;
  scrollbar-color: var(--workflow-term-scroll-thumb) var(--workflow-term-scroll-track);
}

.tool-workflow-terminal-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
}

.tool-workflow-terminal-exit-code {
  color: var(--workflow-term-muted);
  font-size: 11px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-main--patch {
  white-space: pre-wrap;
  word-break: break-word;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-patch-line {
  display: block;
  padding: 1px 4px;
  border-radius: 5px;
}

.tool-workflow-patch-line.is-meta {
  color: var(--workflow-term-muted);
}

.tool-workflow-patch-line.is-note {
  color: var(--workflow-term-muted);
  font-weight: 600;
}

.tool-workflow-patch-line.is-add {
  color: #bbf7d0;
  background: rgba(22, 101, 52, 0.44);
  border-left: 2px solid rgba(74, 222, 128, 0.62);
}

.tool-workflow-patch-line.is-delete {
  color: #fecaca;
  background: rgba(127, 29, 29, 0.5);
  border-left: 2px solid rgba(248, 113, 113, 0.56);
}

.tool-workflow-patch-line.is-move,
.tool-workflow-patch-line.is-update {
  color: #bfdbfe;
  background: rgba(30, 64, 175, 0.36);
}

.tool-workflow-patch-line.is-error {
  color: #fecaca;
  background: rgba(153, 27, 27, 0.48);
}
</style>
