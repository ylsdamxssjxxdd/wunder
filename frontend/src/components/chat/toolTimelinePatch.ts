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

/** 组装补丁卡片视图；非补丁工具或没有补丁原文时返回 null。 */
export const buildTimelinePatchView = (
  run: RawToolRun,
  t: (key: string, params?: Record<string, unknown>) => string
): ToolWorkflowPatchView | null => {
  if (!isApplyPatchToolName(run.toolName)) return null;
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
};
