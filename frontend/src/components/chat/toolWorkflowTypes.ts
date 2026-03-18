import type { CompactionView } from '@/utils/chatCompactionUi';

export type ToolWorkflowCommandView = {
  command: string;
  shell: string;
  terminalText: string;
  exitCode: number | null;
  showExitCode?: boolean;
  metrics?: ToolWorkflowStructuredMetric[];
  streams?: ToolWorkflowCommandStream[];
  previewBody?: string;
};

export type ToolWorkflowPatchLine = {
  key: string;
  kind: 'meta' | 'note' | 'add' | 'delete' | 'move' | 'update' | 'error';
  text: string;
};

export type ToolWorkflowCommandStream = {
  key: string;
  label: string;
  body: string;
  tone?: 'default' | 'danger';
};

export type ToolWorkflowStructuredMetric = {
  key: string;
  label: string;
  value: string;
  tone?: 'default' | 'success' | 'warning';
};

export type ToolWorkflowStructuredRow = {
  key: string;
  title: string;
  meta?: string;
  body?: string;
  mono?: boolean;
  tone?: 'default' | 'success' | 'warning' | 'danger';
};

export type ToolWorkflowStructuredGroup = {
  key: string;
  title?: string;
  rows: ToolWorkflowStructuredRow[];
};

export type ToolWorkflowStructuredView = {
  variant: 'read' | 'list' | 'search' | 'write';
  metrics: ToolWorkflowStructuredMetric[];
  groups: ToolWorkflowStructuredGroup[];
};

export type ToolWorkflowPatchFileView = {
  key: string;
  title: string;
  meta?: string;
  lines: ToolWorkflowPatchLine[];
  tone?: 'default' | 'success' | 'warning' | 'danger';
};

export type ToolWorkflowPatchView = {
  metrics: ToolWorkflowStructuredMetric[];
  files: ToolWorkflowPatchFileView[];
};

export type ToolWorkflowDetailSection = {
  key: string;
  title: string;
  kind: 'text' | 'command' | 'patch' | 'compaction' | 'structured';
  summary?: string;
  body: string;
  commandView: ToolWorkflowCommandView | null;
  patchLines: ToolWorkflowPatchLine[];
  compactionView?: CompactionView | null;
  structuredView?: ToolWorkflowStructuredView | null;
  patchView?: ToolWorkflowPatchView | null;
  empty?: boolean;
};
