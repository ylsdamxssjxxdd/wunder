import type { MessengerSection } from '@/stores/sessionHub';

export type DesktopUpdateState = {
  phase?: string;
  currentVersion?: string;
  latestVersion?: string;
  downloaded?: boolean;
  progress?: number;
  message?: string;
};

export type DesktopInstallResult = {
  ok?: boolean;
  state?: DesktopUpdateState;
};

export type DesktopBridge = {
  toggleDevTools?: () => Promise<boolean> | boolean;
  checkForUpdates?: () => Promise<DesktopUpdateState> | DesktopUpdateState;
  getUpdateState?: () => Promise<DesktopUpdateState> | DesktopUpdateState;
  installUpdate?: () => Promise<DesktopInstallResult | boolean> | DesktopInstallResult | boolean;
  chooseDirectory?: (defaultPath?: string) => Promise<string | null> | string | null;
};

export const DEFAULT_AGENT_KEY = '__default__';
export const USER_CONTAINER_ID = 0;
export const AGENT_CONTAINER_IDS = Array.from({ length: 10 }, (_, index) => index + 1);
export const USER_WORLD_UPLOAD_BASE = 'user-world';
export const WORLD_UPLOAD_SIZE_LIMIT = 200 * 1024 * 1024;
export const WORLD_QUICK_EMOJI_STORAGE_KEY = 'wunder_world_quick_emoji';
export const WORLD_COMPOSER_HEIGHT_STORAGE_KEY = 'wunder_world_composer_height';
export const DISMISSED_AGENT_STORAGE_PREFIX = 'messenger_dismissed_agent_conversations';
export const AGENT_TOOL_OVERRIDE_NONE = '__no_tools__';
export const WORLD_EMOJI_CATALOG = [
  '😀',
  '😁',
  '😂',
  '🤣',
  '😊',
  '😉',
  '😍',
  '😘',
  '😎',
  '🤖',
  '🫡',
  '🤔',
  '🤩',
  '🥳',
  '😴',
  '🤯',
  '😭',
  '😤',
  '🤝',
  '👍',
  '👏',
  '🙏',
  '💪',
  '🎉',
  '🌟',
  '🔥',
  '💡',
  '📌',
  '📎',
  '✅',
  '❓',
  '❗'
];

export const sectionRouteMap: Record<MessengerSection, string> = {
  messages: 'chat',
  users: 'user-world',
  groups: 'user-world',
  agents: 'home',
  tools: 'tools',
  files: 'workspace',
  more: 'settings'
};

export const MESSENGER_SEND_KEY_STORAGE_KEY = 'messenger_send_key';
export const MESSENGER_UI_FONT_SIZE_STORAGE_KEY = 'messenger_ui_font_size';
export const MESSENGER_AGENT_APPROVAL_MODE_STORAGE_KEY = 'messenger_agent_approval_mode';
export const AGENT_MAIN_READ_AT_STORAGE_PREFIX = 'messenger_agent_main_read_at';
export const AGENT_MAIN_UNREAD_STORAGE_PREFIX = 'messenger_agent_main_unread';
export const UNIT_UNGROUPED_ID = '__ungrouped__';

export type AgentLocalCommand = 'new' | 'stop' | 'help' | 'compact';

export type MixedConversation = {
  key: string;
  kind: 'agent' | 'direct' | 'group';
  sourceId: string;
  agentId: string;
  title: string;
  preview: string;
  unread: number;
  lastAt: number;
};

export type ToolEntry = {
  name: string;
  description: string;
  ownerId: string;
  source: Record<string, unknown>;
};

export type AgentFileContainer = {
  id: number;
  agentIds: string[];
  agentNames: string[];
  preview: string;
  primaryAgentId: string;
};

export type AgentOverviewCard = {
  id: string;
  name: string;
  description: string;
  shared: boolean;
  isDefault: boolean;
  runtimeState: AgentRuntimeState;
  hasCron: boolean;
};

export type UnitTreeNode = {
  id: string;
  label: string;
  parentId: string;
  sortOrder: number;
  children: UnitTreeNode[];
};

export type UnitTreeRow = {
  id: string;
  label: string;
  depth: number;
  count: number;
  hasChildren: boolean;
  expanded: boolean;
};

export type WorldHistoryCategory = 'all' | 'media' | 'document' | 'other_file';

export type WorldHistoryRecord = {
  key: string;
  messageId: number;
  sender: string;
  createdAt: number;
  preview: string;
  rawContent: string;
  category: Exclude<WorldHistoryCategory, 'all'> | 'text';
  icon: string;
};

export type AgentRuntimeState = 'idle' | 'running' | 'done' | 'pending' | 'error';
export type MessengerSendKeyMode = 'enter' | 'ctrl_enter';
export type AgentApprovalMode = 'suggest' | 'auto_edit' | 'full_auto';
export type MessengerPerfTrace = {
  label: string;
  startedAt: number;
  marks: Array<{ name: string; at: number }>;
  meta?: Record<string, unknown>;
};

export type FileContainerMenuTarget = {
  scope: 'user' | 'agent';
  id: number;
};

export type WorldComposerViewRef = {
  getComposerElement: () => HTMLElement | null;
  getTextareaElement: () => HTMLTextAreaElement | null;
  getUploadInputElement: () => HTMLInputElement | null;
};
