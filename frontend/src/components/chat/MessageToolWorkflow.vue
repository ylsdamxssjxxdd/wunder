<template>
  <details v-if="shouldRender" class="message-tool-workflow">
    <summary>
      <span class="tool-workflow-title">{{ t('chat.toolWorkflow.title') }}</span>
      <span v-if="latestEntry" class="tool-workflow-latest" :title="latestEntry.summaryTitle">
        {{ latestEntry.summaryTitle }}
      </span>
      <span v-else class="tool-workflow-spacer" />
    </summary>

    <div class="tool-workflow-list">
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
          <span :class="['tool-workflow-entry-status', `is-${entry.status}`]">{{ entry.statusLabel }}</span>
        </summary>

        <div class="tool-workflow-entry-body">
          <div v-if="entry.resultNote" class="tool-workflow-note">{{ entry.resultNote }}</div>

          <ul v-if="entry.patchEntries.length" class="tool-workflow-patch-list">
            <li
              v-for="patch in entry.patchEntries"
              :key="patch.key"
              :class="['tool-workflow-patch-item', `tool-workflow-patch-item--${patch.kind}`]"
            >
              <span class="tool-workflow-patch-sign">{{ patch.sign }}</span>
              <span class="tool-workflow-patch-path">{{ patch.text }}</span>
            </li>
          </ul>

          <div v-if="entry.patchDiffBlocks.length" class="tool-workflow-diff-list">
            <div v-for="block in entry.patchDiffBlocks" :key="block.key" class="tool-workflow-diff-block">
              <div class="tool-workflow-diff-title">{{ block.title }}</div>
              <div class="tool-workflow-diff-code">
                <span
                  v-for="line in block.lines"
                  :key="line.key"
                  :class="['tool-workflow-diff-line', `is-${line.kind}`]"
                >
                  {{ line.text }}
                </span>
              </div>
            </div>
          </div>

          <pre v-if="entry.resultBlock" class="tool-workflow-result">{{ entry.resultBlock }}</pre>
          <pre v-if="entry.commandBlock" class="tool-workflow-code">{{ entry.commandBlock }}</pre>
          <pre v-if="entry.outputBlock" class="tool-workflow-output">{{ entry.outputBlock }}</pre>
          <div v-if="entry.errorText" class="tool-workflow-error">{{ entry.errorText }}</div>
        </div>
      </details>
    </div>
  </details>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

type WorkflowItem = {
  id?: string | number;
  title?: string;
  detail?: string;
  status?: string;
  isTool?: boolean;
  eventType?: string;
  toolName?: string;
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
  resultBlock: string;
  commandBlock: string;
  outputBlock: string;
  resultNote: string;
  errorText: string;
  patchEntries: PatchEntry[];
  patchDiffBlocks: PatchDiffBlock[];
};

type RawEntry = {
  key: string;
  toolName: string;
  callItem: WorkflowItem | null;
  outputItem: WorkflowItem | null;
  resultItem: WorkflowItem | null;
};

type Props = {
  items?: WorkflowItem[];
  loading?: boolean;
  visible?: boolean;
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

const PATH_HINT_KEYS = [
  'path',
  'file',
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
  'output_path',
  'outputPath',
  'input_path',
  'inputPath'
];

const props = withDefaults(defineProps<Props>(), {
  items: () => [],
  loading: false,
  visible: false
});

const { t } = useI18n();
const expandedKeys = ref<Set<string>>(new Set());

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

const parseDetailObject = (detail: unknown): UnknownObject | null => {
  if (typeof detail !== 'string') return null;
  const trimmed = detail.trim();
  if (!trimmed || (trimmed[0] !== '{' && trimmed[0] !== '[')) return null;
  try {
    const parsed = JSON.parse(trimmed);
    return asObject(parsed);
  } catch {
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

const truncateMultiline = (text: string, maxLength = 3600): string => {
  const normalized = String(text || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n').trim();
  if (!normalized) return '';
  if (normalized.length <= maxLength) return normalized;
  return `${normalized.slice(0, maxLength)}\n...`;
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

const resolveCommandFromCall = (item: WorkflowItem | null): string => {
  if (!item) return '';
  const detailObject = parseDetailObject(item.detail);
  const args = detailObject ? asObject(detailObject.args) : null;
  return pickString(
    args?.command,
    args?.cmd,
    args?.script,
    detailObject?.command,
    detailObject?.cmd,
    detailObject?.script
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

const resolveCommandFromResult = (item: WorkflowItem | null): string => {
  if (!item) return '';
  const detailObject = parseDetailObject(item.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);
  const firstResult = Array.isArray(dataObject?.results)
    ? (dataObject.results.find((value) => asObject(value)) as UnknownObject | undefined)
    : undefined;
  return pickString(firstResult?.command, dataObject?.command, resultObject?.command);
};

const extractCallArgs = (item: WorkflowItem | null): UnknownObject | null => {
  if (!item) return null;
  const detailObject = parseDetailObject(item.detail);
  if (!detailObject) return null;
  return asObject(detailObject.args) || detailObject;
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
  return truncateSingleLine(`${base} · ${visible.join(', ')}${suffix}`);
};

const extractResultPayload = (
  resultItem: WorkflowItem | null
): { resultObject: UnknownObject | null; dataObject: UnknownObject | null } => {
  const detailObject = parseDetailObject(resultItem?.detail);
  const resultObject = extractToolResultObject(detailObject);
  const dataObject = extractToolResultData(resultObject);
  return { resultObject, dataObject };
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
  if (!sections.length) return buildTextPreview(content, 14, 1800, '    ');

  const fileBlocks = sections.slice(0, 3).map((section) => {
    const path = section.path || '(unknown)';
    const preview = buildTextPreview(section.body, 12, 1400, '    ');
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

const buildWriteFileResultBlock = (resultObject: UnknownObject | null): string => {
  if (!resultObject) return '';
  const path = pickString(resultObject.path);
  const bytes = toInt(resultObject.bytes);
  const parts: string[] = [];
  if (path) parts.push(path);
  if (bytes > 0) parts.push(`${bytes} bytes`);
  return parts.join(' · ');
};

const buildExecuteCommandResultBlock = (
  resultObject: UnknownObject | null,
  dataObject: UnknownObject | null,
  outputBlock: string
): string => {
  const firstResult = Array.isArray(dataObject?.results)
    ? (dataObject.results.find((value) => asObject(value)) as UnknownObject | undefined)
    : undefined;
  if (!firstResult && !resultObject) return '';

  const returnCode = toOptionalInt(
    firstResult?.returncode,
    resultObject?.meta && asObject(resultObject.meta)?.exit_code
  );
  const rows: string[] = [];
  if (returnCode !== null) rows.push(`exit ${returnCode}`);
  const durationMs = toOptionalInt(asObject(resultObject?.meta)?.duration_ms);
  if (durationMs !== null && durationMs > 0) rows.push(`${durationMs}ms`);

  if (!outputBlock) {
    const stderr = pickString(firstResult?.stderr, dataObject?.stderr, resultObject?.stderr);
    const stdout = pickString(firstResult?.stdout, dataObject?.stdout, resultObject?.stdout);
    if (returnCode === null || returnCode === 0) {
      const stdoutPreview = buildTextPreview(stdout, 6, 1200, '    ');
      if (stdoutPreview) rows.push(`stdout\n${stdoutPreview}`);
      const stderrPreview = buildTextPreview(stderr, 3, 480, '    ');
      if (stderrPreview) rows.push(`stderr\n${stderrPreview}`);
    } else {
      const stderrPreview = buildTextPreview(stderr, 6, 1200, '    ');
      if (stderrPreview) rows.push(`stderr\n${stderrPreview}`);
      const stdoutPreview = buildTextPreview(stdout, 3, 480, '    ');
      if (stdoutPreview) rows.push(`stdout\n${stdoutPreview}`);
    }
  }
  return rows.join('\n\n');
};

const buildGenericResultBlock = (resultObject: UnknownObject | null, dataObject: UnknownObject | null): string => {
  if (!resultObject && !dataObject) return '';

  const rows: string[] = [];
  const path = pickString(dataObject?.path, resultObject?.path);
  if (path) rows.push(path);
  const bytes = toInt(dataObject?.bytes, resultObject?.bytes);
  if (bytes > 0) rows.push(`${bytes} bytes`);

  const summary = pickString(dataObject?.summary, resultObject?.summary, dataObject?.message, resultObject?.message);
  if (summary) rows.push(truncateSingleLine(summary, 180));

  if (!rows.length) {
    const dataText = typeof dataObject === 'string' ? dataObject : '';
    if (dataText.trim()) rows.push(buildTextPreview(dataText, 8, 900, '    '));
  }
  return rows.join('\n');
};

const buildResultBlock = (entry: RawEntry, outputBlock: string): string => {
  if (!entry.resultItem) return '';
  if (isApplyPatchTool(entry.toolName)) return '';

  const { resultObject, dataObject } = extractResultPayload(entry.resultItem);

  if (isExecuteCommandTool(entry.toolName)) {
    return buildExecuteCommandResultBlock(resultObject, dataObject, outputBlock);
  }
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
    return buildWriteFileResultBlock(resultObject);
  }
  return buildGenericResultBlock(resultObject, dataObject);
};

const buildOutputBlock = (entry: RawEntry, command: string): string => {
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
  const command = pickString(
    resolveCommandFromCall(entry.callItem),
    resolveCommandFromOutput(entry.outputItem),
    resolveCommandFromResult(entry.resultItem)
  );
  const toolDisplay = entry.toolName || t('chat.workflow.toolUnknown');
  const patchEntries = buildApplyPatchEntries(entry.resultItem, entry.toolName);
  const patchDiffBlocks = buildApplyPatchDiffBlocks(entry.callItem, entry.toolName);
  const pathHints = collectEntryPathHints(entry, patchEntries, patchDiffBlocks);
  const summaryTitle = command ? truncateSingleLine(command) : composeSummaryTitle(toolDisplay, pathHints);
  const status = resolveEntryStatus(entry);
  const outputBlock = buildOutputBlock(entry, command);
  const resultBlock = buildResultBlock(entry, outputBlock);

  return {
    key: entry.key,
    summaryTitle,
    status,
    statusLabel: statusLabel(status),
    resultBlock,
    commandBlock: command ? `$ ${command}` : '',
    outputBlock,
    resultNote: buildApplyPatchResultNote(entry.resultItem, entry.toolName),
    errorText: status === 'failed' ? buildErrorText(entry.resultItem) : '',
    patchEntries,
    patchDiffBlocks
  };
};

const findLastPendingIndex = (rows: RawEntry[]): number => {
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    if (!rows[index].resultItem) return index;
  }
  return -1;
};

const entries = computed<ToolEntryView[]>(() => {
  const rows: RawEntry[] = [];
  const pendingByTool = new Map<string, number[]>();

  const enqueuePending = (toolKey: string, index: number) => {
    if (!pendingByTool.has(toolKey)) pendingByTool.set(toolKey, []);
    pendingByTool.get(toolKey)?.push(index);
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

  props.items.forEach((item, index) => {
    const kind = resolveToolEventKind(item);
    if (!kind) return;

    const toolName = resolveToolName(item);
    const toolKey = toolName.trim().toLowerCase() || '__unknown__';

    if (kind === 'call') {
      const key = String(item.id || `tool-entry-${index}`);
      rows.push({ key, toolName, callItem: item, outputItem: null, resultItem: null });
      enqueuePending(toolKey, rows.length - 1);
      return;
    }

    if (kind === 'output') {
      const targetIndex = pickPendingForOutput(toolKey);
      if (targetIndex >= 0) {
        rows[targetIndex].outputItem = item;
        if (!rows[targetIndex].toolName && toolName) rows[targetIndex].toolName = toolName;
      } else {
        rows.push({
          key: String(item.id || `tool-entry-${index}`),
          toolName,
          callItem: null,
          outputItem: item,
          resultItem: null
        });
      }
      return;
    }

    const targetIndex = pickPendingForResult(toolKey);
    if (targetIndex >= 0) {
      rows[targetIndex].resultItem = item;
      if (!rows[targetIndex].toolName && toolName) rows[targetIndex].toolName = toolName;
    } else {
      rows.push({
        key: String(item.id || `tool-entry-${index}`),
        toolName,
        callItem: null,
        outputItem: null,
        resultItem: item
      });
    }
  });

  return rows.map(buildEntryView);
});

watch(
  entries,
  (nextEntries) => {
    const validKeys = new Set(nextEntries.map((entry) => entry.key));
    const nextExpanded = new Set<string>();
    expandedKeys.value.forEach((key) => {
      if (validKeys.has(key)) nextExpanded.add(key);
    });
    expandedKeys.value = nextExpanded;
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
  border: none;
  background: transparent;
  padding: 6px 0 0;
}

.message-tool-workflow summary {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--chat-muted);
  cursor: pointer;
  font-weight: 600;
  list-style: none;
  font-size: 12px;
}

.message-tool-workflow summary::marker {
  display: none;
}

.message-tool-workflow summary::before {
  content: '>';
  display: inline-block;
  transition: transform 0.2s ease;
}

.message-tool-workflow[open] summary::before {
  transform: rotate(90deg);
}

.tool-workflow-title {
  color: var(--chat-muted);
}

.tool-workflow-spacer {
  flex: 1 1 auto;
}

.tool-workflow-latest {
  flex: 1 1 auto;
  min-width: 0;
  color: var(--chat-text);
  font-size: 12px;
  font-weight: 500;
  opacity: 0.85;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tool-workflow-list {
  margin-top: 6px;
  height: 280px;
  max-height: 280px;
  overflow-y: auto;
  overscroll-behavior: contain;
  scrollbar-gutter: stable;
  padding: 8px;
  border-radius: 12px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.24);
  background: rgba(var(--ui-accent-rgb), 0.06);
}

.tool-workflow-list::-webkit-scrollbar {
  width: 8px;
}

.tool-workflow-empty {
  color: var(--chat-muted);
  font-size: 12px;
  padding: 8px 10px;
  border-radius: 10px;
  border: 1px dashed rgba(var(--ui-accent-rgb), 0.28);
  background: rgba(var(--ui-accent-rgb), 0.08);
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
  border-color: rgba(var(--ui-accent-rgb), 0.34);
  background: rgba(var(--ui-accent-rgb), 0.1);
}

.tool-workflow-entry > summary {
  list-style: none;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  cursor: pointer;
  color: var(--chat-text);
}

.tool-workflow-entry > summary::marker {
  display: none;
}

.tool-workflow-entry > summary::before {
  content: '>';
  font-size: 10px;
  color: var(--chat-muted);
  transition: transform 0.18s ease;
}

.tool-workflow-entry[open] > summary::before {
  transform: rotate(90deg);
}

.tool-workflow-entry-title {
  min-width: 0;
  flex: 1 1 auto;
  font-size: 12px;
  font-weight: 600;
  color: var(--chat-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
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
  background: rgba(96, 165, 250, 0.16);
  border: 1px solid rgba(59, 130, 246, 0.4);
  color: #1d4ed8;
}

.tool-workflow-entry-status.is-completed {
  background: rgba(34, 197, 94, 0.16);
  border: 1px solid rgba(22, 163, 74, 0.42);
  color: #166534;
}

.tool-workflow-entry-status.is-failed {
  background: rgba(248, 113, 113, 0.14);
  border: 1px solid rgba(220, 38, 38, 0.4);
  color: #b91c1c;
}

.tool-workflow-entry-body {
  padding: 0 10px 10px 24px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-note {
  color: var(--chat-muted);
  font-size: 11px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-result,
.tool-workflow-code,
.tool-workflow-output {
  margin: 0;
  padding: 10px;
  border-radius: 10px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.24);
  background: rgba(var(--ui-accent-rgb), 0.08);
  color: var(--chat-text);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 220px;
  overflow: auto;
}

.tool-workflow-result {
  max-height: 240px;
}

.tool-workflow-patch-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}

.tool-workflow-patch-item {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  min-width: 0;
  color: var(--chat-muted);
  font-size: 12px;
}

.tool-workflow-patch-sign {
  width: 16px;
  flex: 0 0 16px;
  text-align: center;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-patch-path {
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-patch-item--add .tool-workflow-patch-sign {
  color: #15803d;
}

.tool-workflow-patch-item--delete .tool-workflow-patch-sign {
  color: #b91c1c;
}

.tool-workflow-patch-item--update .tool-workflow-patch-sign,
.tool-workflow-patch-item--move .tool-workflow-patch-sign {
  color: #2563eb;
}

.tool-workflow-diff-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool-workflow-diff-block {
  border-left: 2px solid rgba(var(--ui-accent-rgb), 0.35);
  padding-left: 8px;
}

.tool-workflow-diff-title {
  color: var(--chat-muted);
  font-size: 11px;
  margin-bottom: 4px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-diff-code {
  margin: 0;
  padding: 8px;
  border-radius: 8px;
  border: 1px solid rgba(var(--ui-accent-rgb), 0.22);
  background: rgba(var(--ui-accent-rgb), 0.08);
  max-height: 180px;
  overflow: auto;
  font-size: 12px;
  line-height: 1.45;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, 'Liberation Mono',
    'Courier New', monospace;
}

.tool-workflow-diff-line {
  display: block;
  white-space: pre-wrap;
  word-break: break-word;
}

.tool-workflow-diff-line.is-add {
  color: #166534;
  background: rgba(34, 197, 94, 0.12);
}

.tool-workflow-diff-line.is-delete {
  color: #991b1b;
  background: rgba(239, 68, 68, 0.11);
}

.tool-workflow-diff-line.is-meta {
  color: var(--chat-muted);
}

.tool-workflow-diff-line.is-omit {
  color: var(--chat-muted);
  opacity: 0.85;
}

.tool-workflow-error {
  font-size: 12px;
  line-height: 1.45;
  color: #991b1b;
  border: 1px solid rgba(220, 38, 38, 0.34);
  background: rgba(248, 113, 113, 0.14);
  border-radius: 8px;
  padding: 8px 10px;
}
</style>
