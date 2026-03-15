export type ToolWorkflowCommandView = {
  command: string;
  shell: string;
  terminalText: string;
  exitCode: number | null;
  showExitCode?: boolean;
};

export type ToolWorkflowPatchLine = {
  key: string;
  kind: 'meta' | 'note' | 'add' | 'delete' | 'move' | 'update' | 'error';
  text: string;
};

export type ToolWorkflowDetailSection = {
  key: string;
  title: string;
  kind: 'text' | 'command' | 'patch';
  summary?: string;
  body: string;
  commandView: ToolWorkflowCommandView | null;
  patchLines: ToolWorkflowPatchLine[];
  empty?: boolean;
};
