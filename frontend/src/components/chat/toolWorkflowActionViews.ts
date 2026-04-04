import type {
  ToolWorkflowCommandStream,
  ToolWorkflowCommandView,
  ToolWorkflowPatchLine,
  ToolWorkflowPatchView,
  ToolWorkflowStructuredMetric
} from './toolWorkflowTypes';

type Translate = (key: string, params?: Record<string, unknown>) => string;

type CommandCardInput = {
  command: string;
  shell: string;
  exitCode: number | null;
  stdout: string;
  stderr: string;
  preview: string;
  workdir: string;
  timeout: string;
  commandCount: number;
  truncatedCommands: number | null;
  totalBytes: string;
  omittedBytes: string;
  errorText: string;
  showExitCode?: boolean;
};

type PatchCounts = {
  changedFiles: number;
  hunks: number;
  added: number;
  updated: number;
  deleted: number;
  moved: number;
};

type PatchFileCard = {
  key: string;
  title: string;
  meta?: string;
  lines: ToolWorkflowPatchLine[];
  tone?: 'default' | 'success' | 'warning' | 'danger';
};

const STREAM_MAX_LINES = 14;
const STREAM_MAX_CHARS = 9000;

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

const compactStream = (text: string, maxLines = STREAM_MAX_LINES, maxChars = STREAM_MAX_CHARS): string => {
  const normalized = String(text || '').replace(/\r\n/g, '\n').replace(/\r/g, '\n').trim();
  if (!normalized) return '';
  const lines = normalized.split('\n');
  let value = normalized;
  if (lines.length > maxLines) {
    const headLines = Math.max(1, Math.floor(maxLines * 0.65));
    const tailLines = Math.max(1, maxLines - headLines);
    const omitted = Math.max(lines.length - headLines - tailLines, 0);
    value = [
      ...lines.slice(0, headLines),
      `... (${omitted} lines omitted)`,
      ...lines.slice(lines.length - tailLines)
    ].join('\n');
  }
  return truncateByMiddle(value, maxChars).value;
};

const buildMetric = (
  key: string,
  label: string,
  value: unknown,
  tone: ToolWorkflowStructuredMetric['tone'] = 'default'
): ToolWorkflowStructuredMetric | null => {
  const text = String(value ?? '').trim();
  if (!text) return null;
  return { key, label, value: text, tone };
};

const buildStream = (
  key: string,
  label: string,
  body: string,
  tone: ToolWorkflowCommandStream['tone'] = 'default'
): ToolWorkflowCommandStream | null => {
  const compacted = compactStream(body);
  if (!compacted) return null;
  return { key, label, body: compacted, tone };
};

export const buildCommandCardView = (input: CommandCardInput, t: Translate): ToolWorkflowCommandView => {
  const metrics = [
    buildMetric(
      'commandCount',
      input.commandCount > 1 ? t('chat.toolWorkflow.detail.commands') : t('chat.toolWorkflow.detail.command'),
      input.commandCount > 1 ? input.commandCount : ''
    ),
    buildMetric('workdir', t('chat.toolWorkflow.detail.workdir'), input.workdir),
    buildMetric('timeout', t('chat.toolWorkflow.detail.timeout'), input.timeout),
    buildMetric('exitCode', t('chat.toolWorkflow.detail.exitCode'), input.exitCode === null ? '' : input.exitCode),
    buildMetric('truncatedCommands', t('chat.toolWorkflow.detail.truncatedCommands'), input.truncatedCommands || '', 'warning'),
    buildMetric('totalBytes', t('chat.toolWorkflow.detail.totalBytes'), input.totalBytes),
    buildMetric('omittedBytes', t('chat.toolWorkflow.detail.omittedBytes'), input.omittedBytes, 'warning')
  ].filter(Boolean) as ToolWorkflowStructuredMetric[];

  const streams = [
    buildStream('stdout', 'stdout', input.stdout, 'default'),
    buildStream('stderr', 'stderr', input.stderr || input.errorText, 'danger')
  ].filter(Boolean) as ToolWorkflowCommandStream[];

  return {
    command: input.command || '',
    shell: input.shell || 'bash',
    terminalText: [input.stdout, input.stderr, input.preview, input.errorText].filter(Boolean).join('\n\n'),
    exitCode: input.exitCode,
    showExitCode: input.showExitCode,
    metrics,
    streams,
    previewBody: streams.length === 0 ? compactStream(input.preview || input.errorText) : ''
  };
};

export const buildCommandResultNote = (view: ToolWorkflowCommandView, t: Translate): string => {
  if (view.exitCode !== null) {
    return `${t('chat.toolWorkflow.detail.exitCode')} ${view.exitCode}`;
  }
  if (view.streams?.some((item) => item.key.includes('stderr') && item.body.trim())) return 'stderr';
  const omittedMetric = view.metrics?.find((item) => item.key === 'omittedBytes')?.value || '';
  if (omittedMetric) return `${t('chat.toolWorkflow.detail.omittedBytes')} ${omittedMetric}`;
  return '';
};

const buildPatchMetrics = (
  counts: PatchCounts,
  t: Translate,
  includeActions: boolean
): ToolWorkflowStructuredMetric[] => {
  const base = [
    buildMetric('changedFiles', t('chat.toolWorkflow.detail.changedFiles'), counts.changedFiles || ''),
    buildMetric('hunks', t('chat.toolWorkflow.detail.hunks'), counts.hunks || '')
  ];
  if (!includeActions) {
    return base.filter(Boolean) as ToolWorkflowStructuredMetric[];
  }
  return [
    ...base,
    buildMetric('added', t('chat.toolWorkflow.detail.added'), counts.added || '', counts.added > 0 ? 'success' : 'default'),
    buildMetric('updated', t('chat.toolWorkflow.detail.updated'), counts.updated || ''),
    buildMetric('deleted', t('chat.toolWorkflow.detail.deleted'), counts.deleted || '', counts.deleted > 0 ? 'warning' : 'default'),
    buildMetric('moved', t('chat.toolWorkflow.detail.moved'), counts.moved || '')
  ].filter(Boolean) as ToolWorkflowStructuredMetric[];
};

export const buildPatchCallView = (
  counts: Pick<PatchCounts, 'changedFiles' | 'hunks'>,
  files: PatchFileCard[],
  t: Translate
): ToolWorkflowPatchView => ({
  metrics: buildPatchMetrics(
    {
      changedFiles: counts.changedFiles,
      hunks: counts.hunks,
      added: 0,
      updated: 0,
      deleted: 0,
      moved: 0
    },
    t,
    false
  ),
  files
});

export const buildPatchResultView = (
  counts: PatchCounts,
  files: PatchFileCard[],
  t: Translate
): ToolWorkflowPatchView => ({
  metrics: buildPatchMetrics(counts, t, true),
  files
});

export const buildPatchResultNote = (counts: PatchCounts, t: Translate): string => {
  const parts: string[] = [];
  if (counts.added > 0) parts.push(`${t('chat.toolWorkflow.detail.added')} ${counts.added}`);
  if (counts.updated > 0) parts.push(`${t('chat.toolWorkflow.detail.updated')} ${counts.updated}`);
  if (counts.deleted > 0) parts.push(`${t('chat.toolWorkflow.detail.deleted')} ${counts.deleted}`);
  if (counts.moved > 0) parts.push(`${t('chat.toolWorkflow.detail.moved')} ${counts.moved}`);
  if (!parts.length && counts.changedFiles > 0) {
    parts.push(`${t('chat.toolWorkflow.detail.changedFiles')} ${counts.changedFiles}`);
  }
  return parts.join(' · ');
};
