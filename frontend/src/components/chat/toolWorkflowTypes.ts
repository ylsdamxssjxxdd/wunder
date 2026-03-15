export type ToolWorkflowCommandView = {
  command: string;
  shell: string;
  terminalText: string;
  exitCode: number | null;
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
  body: string;
  commandView: ToolWorkflowCommandView | null;
  patchLines: ToolWorkflowPatchLine[];
  empty?: boolean;
};
