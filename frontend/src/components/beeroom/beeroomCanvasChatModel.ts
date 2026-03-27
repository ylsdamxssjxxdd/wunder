export type MissionChatMessage = {
  key: string;
  senderName: string;
  senderAgentId: string;
  mention: string;
  body: string;
  meta: string;
  time: number;
  timeLabel: string;
  tone: 'mother' | 'worker' | 'system' | 'user';
};

export type ComposerTargetOption = {
  agentId: string;
  label: string;
  role: 'mother' | 'worker';
};

export type DispatchRuntimeStatus =
  | 'idle'
  | 'queued'
  | 'running'
  | 'awaiting_approval'
  | 'resuming'
  | 'stopped'
  | 'completed'
  | 'failed';

export type DispatchApprovalItem = {
  approval_id: string;
  session_id: string;
  tool: string;
  summary: string;
};
