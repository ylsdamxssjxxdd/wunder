// AI生成
/**
 * 互通节点/影子的纯函数模型（无 Vue 依赖，便于单测与复用）。
 *
 * 契约来源：docs/云端本地互通方案.md §5.1（状态）、§5.3（我的设备）、
 * §6.1（影子白名单）、§6.3（节点切换器与置灰）、§7.1（kind 分级）。
 * 所有列表长度都在这里显式设上限，避免无界 DOM。
 */

import type {
  InterlinkCommandKind,
  InterlinkNode,
  InterlinkNodeStatus,
  InterlinkShadowTreeEntry
} from '@/api/interlink';
import { INTERLINK_CLOUD_NODE_ID, interlinkNodeWorkspaces } from '@/api/interlink';
import type { WorkspaceEntry } from '@/views/messenger/workspace/workspaceFileModel';
import {
  normalizeWorkspaceEntry,
  normalizeWorkspaceRelativePath,
  workspaceBaseName,
  workspaceParentPath
} from '@/views/messenger/workspace/workspaceFileModel';

/** 「我的设备」每次请求的节点数与 DOM 硬上限（分页累加，绝不全量渲染）。 */
export const INTERLINK_NODE_PAGE_SIZE = 20;
export const INTERLINK_NODE_MAX_RENDERED = 60;
/** §5.3 状态面轮询：慢周期（≥15s），面板隐藏/卸载即停表。 */
export const INTERLINK_POLL_INTERVAL_MS = 20_000;
/** 手动刷新的去抖窗口，避免连点打爆 nodes 端点。 */
export const INTERLINK_REFRESH_DEBOUNCE_MS = 500;

/** 单次远程读取上限（§6.4 小文件内联 ≤1MB）。 */
export const REMOTE_READ_MAX_BYTES = 1024 * 1024;
/** 预览渲染上限：超过则降级为可读提示，不做整文件下载。 */
export const REMOTE_PREVIEW_MAX_BYTES = 2 * 1024 * 1024;
/** 命令轮询节奏与超时（§7.3 审批 120s 超时，故 L1 给到 135s）。 */
export const REMOTE_COMMAND_POLL_INTERVAL_MS = 800;
export const REMOTE_COMMAND_TIMEOUT_MS = 60_000;
export const REMOTE_COMMAND_APPROVAL_TIMEOUT_MS = 135_000;

const NODE_STATUSES: InterlinkNodeStatus[] = ['online', 'busy', 'away', 'reconnecting', 'offline'];

/** §7.1 L0/L0+ 只读 kind（能力缺省时的兜底集合）。 */
const READ_ONLY_KINDS: InterlinkCommandKind[] = [
  'node.summary',
  'shadow.refresh',
  'workspace.list',
  'workspace.search',
  'workspace.stat',
  'workspace.read',
  'threads.list',
  'threads.get'
];

/** L1 会话类：默认需要本地审批（§7.1）。 */
const SESSION_KINDS: InterlinkCommandKind[] = [
  'thread.create',
  'thread.message',
  'thread.cancel',
  'thread.answer'
];

export const normalizeNodeStatus = (value: unknown): InterlinkNodeStatus => {
  const raw = String(value || '').trim().toLowerCase();
  return (NODE_STATUSES as string[]).includes(raw) ? (raw as InterlinkNodeStatus) : 'offline';
};

export const isNodeReachable = (node: InterlinkNode | null | undefined): boolean => {
  if (!node) return false;
  if (node.interlink_enabled === false) return false;
  if (node.connected === true) return true;
  const status = normalizeNodeStatus(node.status);
  return status === 'online' || status === 'busy' || status === 'away';
};

/** 「我的设备」只列持久设备节点（蜂窝/舵机）；web 会话与 cloud 节点不进卡片。 */
export const isDeviceNode = (node: InterlinkNode | null | undefined): boolean => {
  const type = String(node?.node_type || '').trim().toLowerCase();
  return type === 'desktop' || type === 'cli';
};

/**
 * §6.3 侧栏工作区行：按「本地工作区」而不是设备区分。每台设备的每个命名工作区
 * 一行（`设备名-工作区名`）；设备没有命名工作区（舵机 / 尚未同步）时整台设备
 * 折为一行默认目录。渲染行数硬上限沿用节点目录的上限。
 */
export type InterlinkWorkspaceRow = {
  key: string;
  deviceId: string;
  /** 空串＝该设备的默认目录（无命名工作区）。 */
  workspaceId: string;
  label: string;
  node: InterlinkNode;
};

export const workspaceRowLabel = (node: InterlinkNode, workspaceName: string): string => {
  const device = String(node.label || node.node_id || '').trim();
  const name = workspaceName.trim();
  return name && device ? `${device}-${name}` : device || name;
};

export const interlinkWorkspaceRows = (nodes: InterlinkNode[]): InterlinkWorkspaceRow[] => {
  const rows: InterlinkWorkspaceRow[] = [];
  nodes.filter(isDeviceNode).slice(0, INTERLINK_NODE_MAX_RENDERED).forEach((node) => {
    const deviceId = String(node.node_id || '').trim();
    if (!deviceId) return;
    const workspaces = interlinkNodeWorkspaces(node).filter((item) => item.workspace_id);
    if (!workspaces.length) {
      rows.push({
        key: `device:${deviceId}`,
        deviceId,
        workspaceId: '',
        label: workspaceRowLabel(node, ''),
        node
      });
      return;
    }
    workspaces.slice(0, INTERLINK_NODE_MAX_RENDERED).forEach((item) => {
      rows.push({
        key: `device:${deviceId}@${item.workspace_id}`,
        deviceId,
        workspaceId: item.workspace_id,
        label: workspaceRowLabel(node, item.name),
        node
      });
    });
  });
  return rows.slice(0, INTERLINK_NODE_MAX_RENDERED);
};

export const isCloudNode = (node: InterlinkNode | null | undefined): boolean =>
  String(node?.node_id || '').trim() === INTERLINK_CLOUD_NODE_ID ||
  String(node?.node_type || '').trim().toLowerCase() === 'server';

/**
 * 能力判定：capabilities 里既有精确 kind，也允许 `workspace.*` / `workspace` 命名空间写法。
 * capabilities 为空视为「未声明」——按 §1.5「默认仅开只读」放行 L0，其余一律置灰。
 */
export const supportsInterlinkKind = (
  node: InterlinkNode | null | undefined,
  kind: InterlinkCommandKind | string
): boolean => {
  const target = String(kind || '').trim();
  if (!node || !target) return false;
  const caps = Array.isArray(node.capabilities) ? node.capabilities.filter(Boolean) : [];
  if (!caps.length) return READ_ONLY_KINDS.includes(target as InterlinkCommandKind);
  if (caps.includes('*') || caps.includes(target)) return true;
  const namespace = target.split('.')[0];
  return caps.includes(`${namespace}.*`) || caps.includes(namespace);
};

export type InterlinkDisabledReason = 'disabled' | 'offline' | 'capability' | 'no_node';

/** §6.3：离线或能力缺失的操作要置灰并给出原因文案。 */
export const resolveDisabledReason = (
  node: InterlinkNode | null | undefined,
  kind: InterlinkCommandKind | string
): InterlinkDisabledReason | null => {
  if (!node) return 'no_node';
  if (node.interlink_enabled === false) return 'disabled';
  if (!isNodeReachable(node)) return 'offline';
  if (!supportsInterlinkKind(node, kind)) return 'capability';
  return null;
};

export const DISABLED_REASON_KEY: Record<InterlinkDisabledReason, string> = {
  disabled: 'interlink.reason.disabled',
  offline: 'interlink.reason.offline',
  capability: 'interlink.reason.capability',
  no_node: 'interlink.reason.noNode'
};

export const isSessionKind = (kind: InterlinkCommandKind | string): boolean =>
  SESSION_KINDS.includes(String(kind) as InterlinkCommandKind);

/** 状态点样式类：五态各有独立色，样式见 styles/pages/interlink.css。 */
export const statusDotClass = (status: unknown): string => `is-${normalizeNodeStatus(status)}`;

export const statusLabelKey = (status: unknown): string =>
  `interlink.status.${normalizeNodeStatus(status)}`;

export const nodeTypeLabelKey = (type: unknown): string => {
  const raw = String(type || '').trim().toLowerCase();
  return ['web', 'desktop', 'cli', 'server'].includes(raw)
    ? `interlink.nodeType.${raw}`
    : 'interlink.nodeType.unknown';
};

export const nodeTypeIcon = (type: unknown): string => {
  const raw = String(type || '').trim().toLowerCase();
  if (raw === 'desktop') return 'fa-solid fa-display';
  if (raw === 'cli') return 'fa-solid fa-terminal';
  if (raw === 'web') return 'fa-solid fa-globe';
  if (raw === 'server') return 'fa-solid fa-cloud';
  return 'fa-solid fa-device';
};

const parseTime = (value: unknown): number => {
  const raw = String(value || '').trim();
  if (!raw) return Number.NaN;
  const numeric = Number(raw);
  // 兼容秒级时间戳（§4.2 帧 ts 为秒）。
  if (Number.isFinite(numeric) && numeric > 1_000_000_000) {
    return numeric < 100_000_000_000 ? numeric * 1000 : numeric;
  }
  const normalized = raw.includes('T') ? raw : raw.replace(' ', 'T');
  const parsed = Date.parse(normalized);
  return Number.isFinite(parsed) ? parsed : Number.NaN;
};

/** 分钟差：影子水印「更新于 X 分钟前」；无有效时间返回 null（渲染为「未知」）。 */
export const minutesSince = (value: unknown, now = Date.now()): number | null => {
  const ms = parseTime(value);
  if (!Number.isFinite(ms)) return null;
  return Math.max(0, Math.floor((now - ms) / 60_000));
};

/**
 * 影子目录树（§6.1 相对路径白名单）→ 按父目录分组的 WorkspaceEntry，
 * 直接喂给既有工作区树 store 与行渲染器。
 */
export const groupShadowTreeByDirectory = (
  entries: InterlinkShadowTreeEntry[]
): Map<string, WorkspaceEntry[]> => {
  const groups = new Map<string, WorkspaceEntry[]>();
  const seen = new Set<string>();
  (Array.isArray(entries) ? entries : []).forEach((item) => {
    const path = normalizeWorkspaceRelativePath(item?.path);
    if (!path || seen.has(path)) return;
    seen.add(path);
    const parent = workspaceParentPath(path) || '';
    const kind = String(item.kind || '').trim().toLowerCase();
    const entry: WorkspaceEntry = {
      name: workspaceBaseName(path),
      path,
      kind: kind === 'dir' || kind === 'directory' ? 'dir' : 'file',
      size: Number.isFinite(Number(item.size)) ? Number(item.size) : 0,
      updatedTime: String(item.mtime || ''),
      loadedAt: Date.now()
    };
    const bucket = groups.get(parent);
    if (bucket) bucket.push(entry);
    else groups.set(parent, [entry]);
    // 目录节点自身也要有占位条目，父目录才能把它画成可展开的行。
    if (entry.kind === 'dir' && !groups.has(path)) groups.set(path, []);
  });
  return groups;
};

export const sortEntriesByName = (entries: WorkspaceEntry[]): WorkspaceEntry[] =>
  entries.slice().sort((left, right) => {
    if (left.kind !== right.kind) return left.kind === 'dir' ? -1 : 1;
    return left.name.localeCompare(right.name, undefined, { numeric: true });
  });

/**
 * 远程 `workspace.list` 的行 → 既有工作区行模型。
 * 兼容契约里的 `kind`/`mtime` 与云端列表的 `type`/`updated_time` 两种写法。
 */
export const normalizeRemoteDirectoryEntries = (
  rows: Record<string, unknown>[]
): WorkspaceEntry[] => {
  const result: WorkspaceEntry[] = [];
  const seen = new Set<string>();
  (Array.isArray(rows) ? rows : []).forEach((row) => {
    const entry = normalizeWorkspaceEntry({
      path: row.path ?? row.relative_path,
      name: row.name,
      type: row.kind ?? row.type,
      size: row.size,
      updated_time: row.mtime ?? row.updated_time ?? row.modified_time
    });
    if (!entry || seen.has(entry.path)) return;
    seen.add(entry.path);
    result.push(entry);
  });
  return sortEntriesByName(result);
};
