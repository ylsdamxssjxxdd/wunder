/**
 * B3 · 时间线补丁卡片的数据装配。
 *
 * 只做一件事：把 `apply_patch` 的调用参数（`*** Begin Patch` 语法）折成
 * `MessageToolWorkflowPatchSection` 需要的 `ToolWorkflowPatchView`。
 *
 * 边界：
 * - 单次解析的输入有界（`PATCH_INPUT_LIMIT`），流式期间不会把整段补丁文本
 *   反复解析进热路径；
 * - 每文件最多保留 `PATCH_LINE_LIMIT` 行，其余交由卡片组件折叠展示；
 * - 只输出工作区相对路径，不下发服务端绝对路径。
 */

import type { RawToolRun } from './toolWorkflowRunModel';
import { extractWorkflowCallArgs } from './toolWorkflowCallDebug';
import type {
  ToolWorkflowPatchFileView,
  ToolWorkflowPatchLine,
  ToolWorkflowPatchView
} from './toolWorkflowTypes';

/** 单次解析的补丁文本上限（字符）。 */
export const PATCH_INPUT_LIMIT = 200_000;
/** 单文件最多保留的 diff 行数。 */
export const PATCH_LINE_LIMIT = 600;
/** 单卡片最多保留的文件数。 */
export const PATCH_FILE_LIMIT = 8;
/** 文本编辑参数单字段上限（字符）。 */
const EDIT_INPUT_LIMIT = 60_000;

type UnknownObject = Record<string, unknown>;

const asObject = (value: unknown): UnknownObject | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as UnknownObject;
};

const pickString = (...candidates: unknown[]): string => {
  for (const candidate of candidates) {
    if (typeof candidate === 'string' && candidate.trim()) return candidate.trim();
  }
  return '';
};

export const isApplyPatchToolName = (toolName: unknown): boolean => {
  const normalized = String(toolName || '').trim().toLowerCase();
  return normalized === 'apply_patch' || String(toolName || '').includes('\u5e94\u7528\u8865\u4e01');
};

const normalizePatchPath = (value: unknown): string => {
  const normalized = String(value || '')
    .trim()
    .replace(/\\/g, '/')
    .replace(/\/{2,}/g, '/')
    .replace(/^\.\//, '');
  if (!normalized) return '';
  if (normalized.startsWith('/') || /^[a-zA-Z]:\//.test(normalized) || normalized.includes('..')) {
    return '';
  }
  return normalized;
};

const parseJsonLike = (value: unknown): UnknownObject | null => {
  const direct = asObject(value);
  if (direct) return direct;
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed || (trimmed[0] !== '{' && trimmed[0] !== '[')) return null;
  try {
    return asObject(JSON.parse(trimmed));
  } catch {
    return null;
  }
};

const decodeEscapedText = (value: string): string => {
  try {
    return JSON.parse(`"${value}"`);
  } catch {
    return value.replace(/\\n/g, '\n').replace(/\\r/g, '\r').replace(/\\t/g, '\t').replace(/\\"/g, '"');
  }
};

/** 从工具调用参数里取补丁原文（有界扫描）。 */
export const resolvePatchInputText = (run: RawToolRun): string => {
  const sources = [run.callItem, run.resultItem, run.outputItem];
  for (const item of sources) {
    if (!item) continue;
    const raw = pickString(
      item.toolCallRawDetail,
      item.tool_call_raw_detail
    );
    const decoded = raw ? decodeEscapedText(raw).slice(0, PATCH_INPUT_LIMIT) : '';
    if (decoded.includes('*** Begin Patch')) return decoded;
    const parsed = parseJsonLike(item.detail);
    if (parsed) {
      const args = asObject(parsed.arguments) || asObject(parsed.args) || parsed;
      const text = pickString(args?.patch, args?.input, args?.content, args?.patch_text, args?.patchText);
      if (text.includes('*** Begin Patch')) return text.slice(0, PATCH_INPUT_LIMIT);
    }
    if (typeof item.detail === 'string' && item.detail.includes('*** Begin Patch')) {
      return item.detail.slice(0, PATCH_INPUT_LIMIT);
    }
  }
  return '';
};

type PatchFileDraft = {
  action: 'add' | 'delete' | 'update' | 'move' | 'other';
  path: string;
  toPath: string;
  lines: ToolWorkflowPatchLine[];
  omitted: number;
};

/**
 * 解析 `*** Begin Patch` 语法。
 * 与既有实现的差别：只保留有界的行窗口，逐行分类为 add/delete/context/meta。
 */
export const parseApplyPatchText = (patchText: string, t: (key: string, params?: Record<string, unknown>) => string): PatchFileDraft[] => {
  const normalized = String(patchText || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  if (!normalized.includes('*** Begin Patch')) return [];

  const drafts: PatchFileDraft[] = [];
  let current: PatchFileDraft | null = null;
  let lineIndex = 0;

  const flush = () => {
    if (current && (current.path || current.toPath)) {
      if (drafts.length < PATCH_FILE_LIMIT) drafts.push(current);
    }
    current = null;
  };

  const pushLine = (kind: ToolWorkflowPatchLine['kind'], text: string) => {
    if (!current) return;
    if (current.lines.length >= PATCH_LINE_LIMIT) {
      current.omitted += 1;
      return;
    }
    lineIndex += 1;
    current.lines.push({ key: `line-${lineIndex}`, kind, text });
  };

  for (const row of normalized.split('\n')) {
    const addMatch = /^\*\*\* Add File:\s*(.+)\s*$/.exec(row);
    if (addMatch) {
      flush();
      current = { action: 'add', path: addMatch[1].trim(), toPath: '', lines: [], omitted: 0 };
      continue;
    }
    const updateMatch = /^\*\*\* Update File:\s*(.+)\s*$/.exec(row);
    if (updateMatch) {
      flush();
      current = { action: 'update', path: updateMatch[1].trim(), toPath: '', lines: [], omitted: 0 };
      continue;
    }
    const deleteMatch = /^\*\*\* Delete File:\s*(.+)\s*$/.exec(row);
    if (deleteMatch) {
      flush();
      current = { action: 'delete', path: deleteMatch[1].trim(), toPath: '', lines: [], omitted: 0 };
      continue;
    }
    const moveMatch = /^\*\*\* Move to:\s*(.+)\s*$/.exec(row);
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
    if (row.startsWith('@@')) {
      pushLine('meta', `@@ ${row.slice(2).trim()}`);
      continue;
    }
    if (row.startsWith('+') && !row.startsWith('+++')) {
      pushLine('add', `+${row.slice(1)}`);
      continue;
    }
    if (row.startsWith('-') && !row.startsWith('---')) {
      pushLine('delete', `-${row.slice(1)}`);
      continue;
    }
    if (row.startsWith(' ')) {
      pushLine('context', row);
      continue;
    }
    if (row.trim()) {
      pushLine('context', ` ${row.trim()}`);
    }
  }
  flush();

  return drafts.map((draft) => {
    if (draft.lines.length > 0) return draft;
    return { ...draft, lines: [{ key: 'empty', kind: 'note', text: t('chat.timeline.patch.noInlineDiff') }] };
  });
};

const buildFileTone = (action: PatchFileDraft['action']): ToolWorkflowPatchFileView['tone'] =>
  action === 'delete' ? 'danger' : 'default';

const buildFileMeta = (action: PatchFileDraft['action'], t: (key: string, params?: Record<string, unknown>) => string): string => {
  if (action === 'add') return t('chat.timeline.patch.actionAdd');
  if (action === 'delete') return t('chat.timeline.patch.actionDelete');
  if (action === 'move') return t('chat.timeline.patch.actionMove');
  return t('chat.timeline.patch.actionUpdate');
};

// ---------------------------------------------------------------------------
// 文本编辑 / 写入文件的 diff 视图
//
// 这些工具的调用参数直接携带 old/new 文本（`old_string`/`new_string`、
// `old_str`/`new_str`、`file_text`、`content`），把它折成与 apply_patch
// 同一张 diff 卡片：删除行红色、新增行绿色、头部带 +/- 统计。
// ---------------------------------------------------------------------------

type EditLikeArgs = {
  path: string;
  kind: 'replace' | 'create' | 'insert' | 'write';
  oldText: string;
  newText: string;
  insertLine: number | null;
  replaceAll: boolean;
};

const normalizeToolToken = (value: unknown): string => String(value || '').trim().toLowerCase();

const EDIT_TOOL_TOKENS = [
  'text_edit',
  '\u6587\u672c\u7f16\u8f91',
  'str_replace_editor',
  'str_replace_based_edit_tool',
  'edit_file'
] as const;

const WRITE_TOOL_TOKENS = ['write_file', '\u5199\u5165\u6587\u4ef6'] as const;

const matchesAnyToken = (tokens: readonly string[], ...names: Array<unknown>): boolean =>
  names.some((name) => {
    const normalized = normalizeToolToken(name);
    return Boolean(normalized) && tokens.some((token) => normalized === token || normalized.includes(token));
  });

/** 时间线里可折成 diff 卡片的编辑类工具（含写入文件）。 */
export const isTimelineEditToolRun = (run: RawToolRun): boolean =>
  matchesAnyToken(EDIT_TOOL_TOKENS, run.toolName, run.toolRuntimeName, run.toolDisplayName, run.toolFunctionName) ||
  matchesAnyToken(WRITE_TOOL_TOKENS, run.toolName, run.toolRuntimeName, run.toolDisplayName, run.toolFunctionName);

const asPlainObject = (value: unknown): UnknownObject | null => {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null;
  return value as UnknownObject;
};

const parseJsonObject = (value: unknown): UnknownObject | null => {
  const direct = asPlainObject(value);
  if (direct) return direct;
  if (typeof value !== 'string') return null;
  const trimmed = value.trim();
  if (!trimmed || (trimmed[0] !== '{' && trimmed[0] !== '[')) return null;
  try {
    return asPlainObject(JSON.parse(trimmed));
  } catch {
    return null;
  }
};

/** 调用参数候选：调用记录优先，持久化结果行里保留的 invocation JSON 兜底。 */
const collectCallArgCandidates = (run: RawToolRun): UnknownObject[] => {
  const candidates: UnknownObject[] = [];
  [run.callItem, run.resultItem, run.outputItem].forEach((item) => {
    if (!item) return;
    const explicit = extractWorkflowCallArgs(item);
    if (explicit) candidates.push(explicit);
    const raw = parseJsonObject(item.toolCallRawDetail ?? item.tool_call_raw_detail);
    if (raw) candidates.push(raw);
  });
  return candidates;
};

const readArgsString = (source: UnknownObject, keys: readonly string[]): string => {
  for (const key of keys) {
    const value = source[key];
    if (typeof value === 'string') return value;
  }
  return '';
};

const readArgsInt = (source: UnknownObject, keys: readonly string[]): number | null => {
  for (const key of keys) {
    const value = source[key];
    if (typeof value === 'number' && Number.isFinite(value)) return Math.trunc(value);
    if (typeof value === 'string') {
      const parsed = Number.parseInt(value.trim(), 10);
      if (Number.isFinite(parsed)) return parsed;
    }
  }
  return null;
};

const resolveEditLikeArgs = (run: RawToolRun): EditLikeArgs | null => {
  const isEdit = matchesAnyToken(
    EDIT_TOOL_TOKENS,
    run.toolName,
    run.toolRuntimeName,
    run.toolDisplayName,
    run.toolFunctionName
  );
  const isWrite = matchesAnyToken(
    WRITE_TOOL_TOKENS,
    run.toolName,
    run.toolRuntimeName,
    run.toolDisplayName,
    run.toolFunctionName
  );
  if (!isEdit && !isWrite) return null;

  for (const args of collectCallArgCandidates(run)) {
    const path = readArgsString(args, ['file_path', 'path', 'filename', 'file']);
    if (isWrite) {
      const content = readArgsString(args, ['content', 'file_text', 'text']);
      if (path && content) {
        return { path, kind: 'write', oldText: '', newText: content, insertLine: null, replaceAll: false };
      }
      continue;
    }
    // str_replace_editor 分支带 `command` 子命令；text_edit 直带 old/new。
    const command = normalizeToolToken(args.command);
    if (command === 'view') continue;
    if (command === 'create') {
      const fileText = readArgsString(args, ['file_text', 'content', 'text']);
      if (path && fileText) {
        return { path, kind: 'create', oldText: '', newText: fileText, insertLine: null, replaceAll: false };
      }
      continue;
    }
    if (command === 'insert') {
      const text = readArgsString(args, ['new_str', 'new_string', 'insert_text', 'text']);
      if (path && text) {
        return {
          path,
          kind: 'insert',
          oldText: '',
          newText: text,
          insertLine: readArgsInt(args, ['insert_line', 'insertLine', 'line']),
          replaceAll: false
        };
      }
      continue;
    }
    const hasOld = typeof args.old_string === 'string' || typeof args.old_str === 'string';
    const hasNew = typeof args.new_string === 'string' || typeof args.new_str === 'string';
    if (path && hasOld && hasNew) {
      return {
        path,
        kind: 'replace',
        oldText: readArgsString(args, ['old_string', 'old_str']),
        newText: readArgsString(args, ['new_string', 'new_str']),
        insertLine: null,
        replaceAll: args.replace_all === true || args.replaceAll === true
      };
    }
  }
  return null;
};

const splitDiffLines = (text: string, limit: number): { lines: string[]; omitted: number } => {
  const normalized = String(text || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  if (!normalized) return { lines: [], omitted: 0 };
  const parts = normalized.split('\n');
  // 结尾换行产生的空尾行不是一行内容。
  if (parts.length > 1 && parts[parts.length - 1] === '') parts.pop();
  if (parts.length <= limit) return { lines: parts, omitted: 0 };
  return { lines: parts.slice(0, limit), omitted: parts.length - limit };
};

/** 把编辑参数折成与 apply_patch 同构的 diff 卡片视图；非编辑工具返回 null。 */
export const buildTimelineEditPatchView = (
  run: RawToolRun,
  t: (key: string, params?: Record<string, unknown>) => string
): ToolWorkflowPatchView | null => {
  const edit = resolveEditLikeArgs(run);
  if (!edit) return null;
  const oldText = edit.oldText.slice(0, EDIT_INPUT_LIMIT);
  const newText = edit.newText.slice(0, EDIT_INPUT_LIMIT);
  if (edit.kind === 'replace' && oldText === newText) return null;
  if (!oldText && !newText) return null;

  const removed = splitDiffLines(oldText, PATCH_LINE_LIMIT);
  const added = splitDiffLines(newText, PATCH_LINE_LIMIT);
  const lines: ToolWorkflowPatchLine[] = [];
  let lineIndex = 0;
  const pushLines = (
    values: string[],
    kind: ToolWorkflowPatchLine['kind'],
    startLine: number | null
  ) => {
    values.forEach((text, offset) => {
      lineIndex += 1;
      lines.push({
        key: `edit-${lineIndex}`,
        kind,
        text,
        oldLine: kind === 'delete' && startLine !== null ? startLine + offset : null,
        newLine: kind === 'add' && startLine !== null ? startLine + offset : null
      });
    });
  };
  pushLines(removed.lines, 'delete', null);
  pushLines(added.lines, 'add', edit.kind === 'insert' && edit.insertLine !== null && edit.insertLine >= 0
    ? edit.insertLine + 1
    : null);

  const omittedLines = removed.omitted + added.omitted;
  const metaParts: string[] = [];
  if (edit.kind === 'create') metaParts.push(t('chat.timeline.patch.actionAdd'));
  else if (edit.kind === 'write') metaParts.push(t('chat.timeline.patch.actionWrite'));
  else if (edit.kind === 'insert') {
    metaParts.push(t('chat.timeline.patch.actionInsert'));
    if (edit.insertLine !== null && edit.insertLine >= 0) {
      metaParts.push(t('chat.timeline.patch.insertAfterLine', { line: edit.insertLine }));
    }
  } else metaParts.push(t('chat.timeline.patch.actionUpdate'));
  if (edit.replaceAll) metaParts.push(t('chat.timeline.patch.replaceAll'));

  const file: ToolWorkflowPatchFileView = {
    key: `edit-${run.key}`,
    title: edit.path,
    meta: metaParts.join(' · '),
    lines,
    omittedLines,
    tone: 'default'
  };
  return {
    metrics: [
      { key: 'changedFiles', label: t('chat.timeline.patch.changedFiles'), value: '1' },
      { key: 'addedLines', label: '+Lines', value: String(added.lines.length), tone: added.lines.length > 0 ? 'success' : 'default' },
      { key: 'deletedLines', label: '-Lines', value: String(removed.lines.length), tone: removed.lines.length > 0 ? 'warning' : 'default' }
    ],
    files: [file]
  };
};

/** 组装补丁卡片视图；非补丁/编辑工具或没有可解析参数时返回 null。 */
export const buildTimelinePatchView = (
  run: RawToolRun,
  t: (key: string, params?: Record<string, unknown>) => string
): ToolWorkflowPatchView | null => {
  if (isApplyPatchToolName(run.toolName)) {
    const patchText = resolvePatchInputText(run);
    if (!patchText) return null;
    const drafts = parseApplyPatchText(patchText, t);
    if (!drafts.length) return null;

    let addedLines = 0;
    let deletedLines = 0;
    const files: ToolWorkflowPatchFileView[] = drafts.map((draft, index) => {
      draft.lines.forEach((line) => {
        if (line.kind === 'add') addedLines += 1;
        if (line.kind === 'delete') deletedLines += 1;
      });
      const from = normalizePatchPath(draft.path);
      const to = normalizePatchPath(draft.toPath);
      const title = from && to && from !== to ? `${from} → ${to}` : from || to || `file-${index + 1}`;
      return {
        key: `patch-${index}-${title}`,
        title,
        meta: buildFileMeta(draft.action, t),
        lines: draft.lines,
        omittedLines: draft.omitted,
        tone: buildFileTone(draft.action)
      };
    });

    return {
      metrics: [
        { key: 'changedFiles', label: t('chat.timeline.patch.changedFiles'), value: String(files.length) },
        { key: 'addedLines', label: '+Lines', value: String(addedLines), tone: addedLines > 0 ? 'success' : 'default' },
        { key: 'deletedLines', label: '-Lines', value: String(deletedLines), tone: deletedLines > 0 ? 'warning' : 'default' }
      ],
      files
    };
  }
  return buildTimelineEditPatchView(run, t);
};
