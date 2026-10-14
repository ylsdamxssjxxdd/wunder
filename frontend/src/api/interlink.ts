// AI生成
/**
 * 互通（interlink）用户面 API 客户端。
 *
 * 契约冻结来源：docs/云端本地互通方案.md §3.2（REST 端点）、§6.1/§6.2（影子）、
 * §7.1（命令 kind 目录与分级）、§7.4（远程会话视图 WS）。服务端按同一份契约实现，
 * 这里不得自行发明路由或字段。
 *
 * 约定：
 * - 路径省略 `/wunder` 前缀（`api/http` 的 baseURL 已带）。
 * - 除 `/interlink/commands/{id}/blob` 外，响应都是 `{ data: ... }` 信封，统一在本层拆包，
 *   组件只拿强类型结果，不再各自 `as any`。
 * - 命令 id 由发起方生成（§4.3 幂等：服务端与本地各 24h 去重窗口），重发不产生二次执行。
 */

import api from './http';
import type { ApiPayload, QueryParams } from './types';

export type InterlinkNodeType = 'web' | 'desktop' | 'cli' | 'server';

/** §5.1 统一状态模型的五个用户可见状态。 */
export type InterlinkNodeStatus = 'online' | 'busy' | 'away' | 'reconnecting' | 'offline';

/** §4.3 命令状态机。`approval pending` 期间停在 `issued`。 */
export type InterlinkCommandStatus =
  | 'issued'
  | 'queued'
  | 'acked'
  | 'running'
  | 'succeeded'
  | 'failed'
  | 'canceled'
  | 'timeout';

/** §7.3 审批状态；服务端字段为字符串，保留未知值以免契约加值时前端崩。 */
export type InterlinkApprovalState =
  | 'none'
  | 'not_required'
  | 'pending'
  | 'approved'
  | 'rejected'
  | 'expired'
  | (string & {});

/** §7.1 操作目录（kind 白名单）。 */
export type InterlinkCommandKind =
  | 'workspace.list'
  | 'workspace.read'
  | 'workspace.search'
  | 'workspace.stat'
  | 'workspace.write'
  | 'workspace.mkdir'
  | 'workspace.move'
  | 'workspace.copy'
  | 'workspace.delete'
  | 'threads.list'
  | 'threads.get'
  | 'thread.create'
  | 'thread.message'
  | 'thread.cancel'
  | 'thread.answer'
  | 'node.summary'
  | 'shadow.refresh'
  | 'tool.exec'
  | 'agent.spawn';

export type InterlinkCommandDirection = 'c2l' | 'l2c' | (string & {});

/** §6.3 本地工作区行：节点 meta 携带的精简列表（id + 名称，来自影子投影）。 */
export type InterlinkNodeWorkspace = {
  workspace_id: string;
  name: string;
};

export type InterlinkNode = {
  node_id: string;
  node_type: InterlinkNodeType | string;
  user_id: string;
  label: string;
  status: InterlinkNodeStatus | string;
  last_seen_at: string;
  capabilities: string[];
  shadow_revision: number;
  connected: boolean;
  meta: Record<string, unknown>;
  /** §5.3 kill switch；契约把它放在节点 meta 里，缺省视为允许。 */
  interlink_enabled: boolean;
};

/** 读取节点 meta 里的本地工作区列表（缺失/损坏一律得到空数组）。 */
export const interlinkNodeWorkspaces = (node: InterlinkNode | null | undefined): InterlinkNodeWorkspace[] => {
  const raw = node?.meta?.workspaces;
  if (!Array.isArray(raw)) return [];
  return raw
    .map((item) => {
      const record = item && typeof item === 'object' ? (item as Record<string, unknown>) : {};
      return {
        workspace_id: String(record.workspace_id ?? '').trim(),
        name: String(record.name ?? '').trim()
      };
    })
    .filter((item) => item.workspace_id !== '' || item.name !== '')
    .slice(0, 20);
};

export type InterlinkNodeListPage = {
  nodes: InterlinkNode[];
  aggregate_status: InterlinkNodeStatus | string;
  online_count: number;
  total: number;
  /**
   * 设备口径计数（§5.3）：只统计持久设备（蜂窝/舵机），不含本页的 web 会话与
   * cloud 节点。`total`/`online_count` 是「节点目录」口径，两者不可混用。
   */
  device_total: number;
  device_online: number;
};

export type InterlinkShadowThread = {
  local_thread_id: string;
  title: string;
  status: string;
  agent: string;
  updated_at: string;
  message_count: number;
  duration_ms: number;
};

export type InterlinkShadowTask = {
  task_id: string;
  schedule: string;
  enabled: boolean;
  next_run_at: string;
};

/** §6.1 目录树投影条目：仅相对路径 / 类型 / 大小 / mtime，无内容无绝对路径。 */
export type InterlinkShadowTreeEntry = {
  path: string;
  kind: string;
  size: number;
  mtime: string;
};

/** §6.1 单个本地工作区的投影：树 + 用量；`workspace_id` 为空表示默认目录。 */
export type InterlinkShadowWorkspace = {
  workspace_id: string;
  name: string;
  tree: InterlinkShadowTreeEntry[];
  truncated: boolean;
  usage: Record<string, unknown>;
};

export type InterlinkShadow = {
  device_id: string;
  revision: number;
  summary: Record<string, unknown> | null;
  threads: InterlinkShadowThread[];
  tasks: InterlinkShadowTask[];
  workspaces: InterlinkShadowWorkspace[];
  synced_at: string;
};

export type InterlinkCommandSubmitResult = {
  command_id: string;
  status: InterlinkCommandStatus | string;
  approval_state: InterlinkApprovalState;
  approval_id: string;
  queued: boolean;
};

export type InterlinkCommandRecord = {
  command_id: string;
  direction: InterlinkCommandDirection;
  kind: string;
  status: InterlinkCommandStatus | string;
  approval_state: InterlinkApprovalState;
  created_at: string;
  acked_at: string;
  finished_at: string;
  error_code: string;
  error_summary: string;
  result: Record<string, unknown> | null;
};

export type InterlinkNodePageParams = {
  limit?: number;
  offset?: number;
  node_type?: string;
  status?: string;
};

export type InterlinkCommandRequest = {
  to: string;
  kind: InterlinkCommandKind | string;
  args?: ApiPayload;
  command_id?: string;
};

export type InterlinkEnabledResult = {
  ok: boolean;
  interlink_enabled: boolean;
};

type ApiEnvelope = { data?: unknown };

type RequestOptions = {
  signal?: AbortSignal;
};

const asRecord = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' ? (value as Record<string, unknown>) : {};

const asText = (value: unknown): string => (typeof value === 'string' ? value : value == null ? '' : String(value));

const asNumber = (value: unknown, fallback = 0): number => {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
};

const asBoolean = (value: unknown, fallback = false): boolean =>
  typeof value === 'boolean' ? value : fallback;

const asList = (value: unknown): Record<string, unknown>[] =>
  Array.isArray(value) ? value.map(asRecord) : [];

const unwrap = (response: unknown): Record<string, unknown> => {
  const envelope = asRecord(response);
  const data = envelope.data;
  if (data && typeof data === 'object') {
    return data as Record<string, unknown>;
  }
  // 极少数端点（历史约定）直接返回裸对象；拆不到 data 时按原样消费。
  return envelope;
};

/** 云端节点在命令与目标里的固定标识（§8.1）。 */
export const INTERLINK_CLOUD_NODE_ID = 'cloud';

/** 命令 `to` / remote_ws `target` 的设备形态：`device:<id>`。 */
export const interlinkDeviceTarget = (deviceId: string): string =>
  `device:${String(deviceId || '').trim()}`;

/** 节点 id → 命令目标；`cloud` 节点直接用 `cloud`（§8.1）。 */
export const interlinkTargetOf = (nodeId: string): string => {
  const normalized = String(nodeId || '').trim();
  if (!normalized) return '';
  return normalized === INTERLINK_CLOUD_NODE_ID ? normalized : interlinkDeviceTarget(normalized);
};

export const isInterlinkDeviceTarget = (target: string): boolean =>
  String(target || '').trim().startsWith('device:');

export const interlinkDeviceIdOf = (target: string): string =>
  String(target || '').trim().replace(/^device:/, '');

const buildCommandId = (): string => {
  const random =
    typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID().replace(/-/g, '').slice(0, 16)
      : `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 10)}`;
  return `cmd_${random}`;
};

const normalizeNode = (raw: Record<string, unknown>): InterlinkNode => {
  const meta = asRecord(raw.meta);
  const enabledRaw = meta.interlink_enabled ?? meta.enabled ?? raw.interlink_enabled;
  return {
    // 服务端把设备节点报成 `device:<id>`；本仓库所有消费方（影子/开关/清除按
    // `/nodes/{device_id}/...` 取数，命令目标另加前缀）都用裸 id，这里统一剥前缀。
    node_id: interlinkDeviceIdOf(asText(raw.node_id)),
    node_type: asText(raw.node_type),
    user_id: asText(raw.user_id),
    label: asText(raw.label),
    status: asText(raw.status),
    last_seen_at: asText(raw.last_seen_at),
    capabilities: Array.isArray(raw.capabilities)
      ? raw.capabilities.map((item) => asText(item)).filter(Boolean)
      : [],
    shadow_revision: asNumber(raw.shadow_revision),
    connected: asBoolean(raw.connected),
    meta,
    interlink_enabled: asBoolean(enabledRaw, true)
  };
};

/** 持久设备节点判定：只有蜂窝/舵机是「设备」，web 会话与 cloud 节点不是。 */
const isDeviceNodeType = (value: unknown): boolean => {
  const type = String(value || '').trim().toLowerCase();
  return type === 'desktop' || type === 'cli';
};

const countDeviceNodes = (nodes: InterlinkNode[]): number =>
  nodes.filter((node) => isDeviceNodeType(node.node_type)).length;

const countReachableDeviceNodes = (nodes: InterlinkNode[]): number =>
  nodes.filter(
    (node) =>
      isDeviceNodeType(node.node_type) &&
      ['online', 'busy'].includes(String(node.status || '').trim().toLowerCase())
  ).length;

export const fetchInterlinkNodes = async (
  params: InterlinkNodePageParams = {},
  options: RequestOptions = {}
): Promise<InterlinkNodeListPage> => {
  // 服务端真的按这四个键过滤分页（`total` 是过滤后的全集大小，不是本页条数）；
  // 上限 60 与本地渲染上限 INTERLINK_NODE_MAX_RENDERED 对齐，服务端上限更宽。
  const query: QueryParams = {
    limit: Math.max(1, Math.min(60, Number(params.limit) || 20)),
    offset: Math.max(0, Number(params.offset) || 0)
  };
  if (params.node_type) query.node_type = params.node_type;
  if (params.status) query.status = params.status;
  const response = await api.get('/interlink/nodes', { params: query, signal: options.signal });
  const data = unwrap(response?.data);
  const nodes = asList(data.nodes).map(normalizeNode);
  return {
    nodes,
    aggregate_status: asText(data.aggregate_status) || 'offline',
    online_count: asNumber(data.online_count),
    total: asNumber(data.total, nodes.length),
    device_total: asNumber(data.device_total, countDeviceNodes(nodes)),
    device_online: asNumber(data.device_online, countReachableDeviceNodes(nodes))
  };
};

const normalizeShadow = (raw: Record<string, unknown>): InterlinkShadow => {
  const workspace = asRecord(raw.workspace);
  const workspaceEntries = Array.isArray(workspace.workspaces) ? workspace.workspaces : [];
  return {
    device_id: asText(raw.device_id),
    revision: asNumber(raw.revision),
    summary: raw.summary ? asRecord(raw.summary) : null,
    threads: asList(raw.threads).map((item) => ({
      local_thread_id: asText(item.local_thread_id),
      title: asText(item.title),
      status: asText(item.status),
      agent: asText(item.agent),
      updated_at: asText(item.updated_at),
      message_count: asNumber(item.message_count),
      duration_ms: asNumber(item.duration_ms)
    })),
    tasks: asList(raw.tasks).map((item) => ({
      task_id: asText(item.task_id),
      schedule: asText(item.schedule),
      enabled: asBoolean(item.enabled),
      next_run_at: asText(item.next_run_at)
    })),
    workspaces: workspaceEntries.slice(0, 20).map((item) => ({
      workspace_id: asText(item.workspace_id),
      name: asText(item.name),
      tree: asList(item.tree).map((entry) => ({
        path: asText(entry.path),
        kind: asText(entry.kind),
        size: asNumber(entry.size),
        mtime: asText(entry.mtime)
      })),
      truncated: item.truncated === true,
      usage: asRecord(item.usage)
    })),
    synced_at: asText(raw.synced_at)
  };
};

/** 影子未同步过时服务端字段为 null、revision 为 0；这里原样返回，由视图区分「空投影」。 */
export const fetchInterlinkShadow = async (
  deviceId: string,
  options: RequestOptions = {}
): Promise<InterlinkShadow> => {
  const response = await api.get(
    `/interlink/nodes/${encodeURIComponent(String(deviceId || '').trim())}/shadow`,
    { signal: options.signal }
  );
  return normalizeShadow(unwrap(response?.data));
};

export const submitInterlinkCommand = async (
  payload: InterlinkCommandRequest
): Promise<InterlinkCommandSubmitResult> => {
  const commandId = String(payload.command_id || '').trim() || buildCommandId();
  const body: ApiPayload = {
    to: String(payload.to || '').trim(),
    kind: String(payload.kind || '').trim(),
    args: payload.args || {},
    command_id: commandId
  };
  const response = await api.post('/interlink/commands', body, { timeout: 20_000 });
  const data = unwrap(response?.data);
  return {
    command_id: asText(data.command_id) || commandId,
    status: asText(data.status) || 'issued',
    approval_state: asText(data.approval_state) || 'none',
    approval_id: asText(data.approval_id),
    queued: data.queued === true
  };
};

export const fetchInterlinkCommand = async (
  commandId: string,
  options: RequestOptions = {}
): Promise<InterlinkCommandRecord> => {
  const response = await api.get(
    `/interlink/commands/${encodeURIComponent(String(commandId || '').trim())}`,
    { signal: options.signal }
  );
  const data = unwrap(response?.data);
  return {
    command_id: asText(data.command_id),
    direction: asText(data.direction),
    kind: asText(data.kind),
    status: asText(data.status) || 'issued',
    approval_state: asText(data.approval_state) || 'none',
    created_at: asText(data.created_at),
    acked_at: asText(data.acked_at),
    finished_at: asText(data.finished_at),
    error_code: asText(data.error_code),
    error_summary: asText(data.error_summary),
    result: data.result ? asRecord(data.result) : null
  };
};

export const cancelInterlinkCommand = async (commandId: string): Promise<boolean> => {
  const response = await api.post(
    `/interlink/commands/${encodeURIComponent(String(commandId || '').trim())}/cancel`,
    {}
  );
  return unwrap(response?.data).ok === true;
};

/** §6.4 大文件走 blob 端点（支持 Range/断点）；返回原始二进制，不拆 JSON 信封。 */
export const fetchInterlinkCommandBlob = async (
  commandId: string,
  options: RequestOptions & { range?: string } = {}
): Promise<Blob> => {
  const headers: Record<string, string> = {};
  if (options.range) headers.Range = options.range;
  const response = await api.get(
    `/interlink/commands/${encodeURIComponent(String(commandId || '').trim())}/blob`,
    { responseType: 'blob', headers, signal: options.signal }
  );
  return response.data as Blob;
};

/** §5.3 kill switch：关闭后服务端拒绝隧道并在 30s 内拆除既有隧道。 */
export const setInterlinkNodeEnabled = async (
  deviceId: string,
  enabled: boolean
): Promise<InterlinkEnabledResult> => {
  const response = await api.patch(
    `/interlink/nodes/${encodeURIComponent(String(deviceId || '').trim())}/enabled`,
    { enabled }
  );
  const data = unwrap(response?.data);
  return { ok: data.ok === true, interlink_enabled: data.interlink_enabled === true };
};

/** §5.3 清空云影子：删除该设备的投影（不触碰本地数据）。 */
export const purgeInterlinkShadow = async (deviceId: string): Promise<boolean> => {
  const response = await api.post(
    `/interlink/nodes/${encodeURIComponent(String(deviceId || '').trim())}/purge_shadow`,
    {}
  );
  return unwrap(response?.data).ok === true;
};

// ---------------------------------------------------------------------------
// §6.3 侧栏工作区行的云端排序（纯蜂巢偏好，不回写本地设备）
// ---------------------------------------------------------------------------

export const WORKSPACE_ORDER_MAX_KEYS = 128;
export const WORKSPACE_ORDER_KEY_MAX_CHARS = 200;

/** 与 api/interlink.rs 的 normalize_workspace_order_keys 同一契约。 */
export const normalizeWorkspaceOrder = (raw: unknown): string[] => {
  const items = Array.isArray(raw) ? raw : [];
  const seen = new Set<string>();
  const keys: string[] = [];
  for (const item of items) {
    // 只收字符串：数字/对象不是宽容转换而是丢弃。
    if (typeof item !== 'string') continue;
    const key = item.trim().slice(0, WORKSPACE_ORDER_KEY_MAX_CHARS);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    keys.push(key);
    if (keys.length >= WORKSPACE_ORDER_MAX_KEYS) break;
  }
  return keys;
};

/** 行 key 有序列表：设备工作区行 `device:<id>@<workspace_id>`，兜底行 `device:<id>`。 */
export const fetchInterlinkWorkspaceOrder = async (
  options: RequestOptions = {}
): Promise<string[]> => {
  const response = await api.get('/interlink/workspace-order', { signal: options.signal });
  return normalizeWorkspaceOrder(unwrap(response?.data).keys);
};

export const saveInterlinkWorkspaceOrder = async (keys: string[]): Promise<void> => {
  await api.put('/interlink/workspace-order', { keys: normalizeWorkspaceOrder(keys) });
};
