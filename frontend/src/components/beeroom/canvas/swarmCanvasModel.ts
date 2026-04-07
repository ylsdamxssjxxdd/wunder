import type {
  BeeroomNodeWorkflowLine,
  BeeroomTaskWorkflowPreview,
  BeeroomWorkflowItem,
  BeeroomWorkflowTone
} from '@/components/beeroom/beeroomTaskWorkflow';
import {
  buildNodeWorkflowPreviewLines,
  buildTaskWorkflowRuntime,
  compareBeeroomMissionTasksByDisplayPriority,
  resolveBeeroomTaskMoment
} from '@/components/beeroom/beeroomTaskWorkflow';
import type { BeeroomCanvasPositionOverride } from '@/components/beeroom/beeroomMissionCanvasStateCache';
import type { BeeroomMissionSubagentItem } from '@/components/beeroom/useBeeroomMissionSubagentPreview';
import type { BeeroomGroup, BeeroomMember, BeeroomMission, BeeroomMissionTask } from '@/stores/beeroom';
import {
  parseAgentAvatarIconConfig,
  resolveAgentAvatarImageByConfig,
  resolveAgentAvatarInitial
} from '@/utils/agentAvatar';

export type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

export type SwarmNodeRole = 'mother' | 'worker' | 'subagent';
export type SwarmNodeEmphasis = 'default' | 'active' | 'dormant';

export type CanvasNodeMeta = {
  id: string;
  agent_id: string;
  agent_name: string;
  role: SwarmNodeRole;
  role_label: string;
  status: string;
  task_total: number;
  active_session_total: number;
  updated_time: number;
  summary: string;
  entry_agent: boolean;
  parent_id: string;
  emphasis: SwarmNodeEmphasis;
};

export type SwarmProjectionNode = {
  id: string;
  agentId: string;
  name: string;
  displayName: string;
  role: SwarmNodeRole;
  roleLabel: string;
  status: string;
  statusLabel: string;
  selected: boolean;
  accentColor: string;
  avatarInitial: string;
  avatarImageUrl: string;
  x: number;
  y: number;
  width: number;
  height: number;
  workflowTaskId: string;
  workflowTone: BeeroomWorkflowTone;
  workflowLines: BeeroomNodeWorkflowLine[];
  parentId: string;
  emphasis: SwarmNodeEmphasis;
  introFromId: string;
  introOrder: number;
};

export type SwarmProjectionEdge = {
  id: string;
  source: string;
  target: string;
  label: string;
  active: boolean;
  selected: boolean;
  kind: 'dispatch' | 'subagent';
};

export type SwarmProjectionBounds = {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
  width: number;
  height: number;
};

export type SwarmProjection = {
  nodes: SwarmProjectionNode[];
  edges: SwarmProjectionEdge[];
  nodeMetaMap: Map<string, CanvasNodeMeta>;
  memberMap: Map<string, BeeroomMember>;
  tasksByAgent: Map<string, BeeroomMissionTask[]>;
  motherNodeId: string;
  bounds: SwarmProjectionBounds;
};

export type BeeroomSwarmDispatchPreview = {
  sessionId: string;
  targetAgentId: string;
  targetName: string;
  status: string;
  summary: string;
  dispatchLabel: string;
  updatedTime: number;
  subagents: BeeroomMissionSubagentItem[];
};

type HoneycombSlot = {
  q: number;
  r: number;
};

type SubagentBranchVector = {
  axisX: number;
  axisY: number;
  perpX: number;
  perpY: number;
};

type SubagentVisualState = {
  status: string;
  statusLabel: string;
  emphasis: SwarmNodeEmphasis;
  workflowTone: BeeroomWorkflowTone;
  accentColor: string;
  active: boolean;
};

export const HONEYCOMB_RADIUS = 186;
export const HONEYCOMB_VERTICAL_RATIO = 1.2;
export const NODE_WIDTH = 272;
export const NODE_HEIGHT = 170;
export const WORLD_PADDING = 96;
export const ACTIVE_DISPATCH_STATUSES = new Set(['queued', 'running', 'awaiting_idle']);

const SUBAGENT_NODE_WIDTH = 224;
const SUBAGENT_NODE_HEIGHT = 142;
const SUBAGENT_BRANCH_BASE_GAP = 158;
const SUBAGENT_BRANCH_ROW_GAP = 128;
const SUBAGENT_BRANCH_LANE_GAP = 174;
const SUBAGENT_ROW_CAPACITY = 3;
const SUBAGENT_ACTIVE_STATUSES = new Set(['running', 'waiting', 'queued', 'accepted', 'cancelling']);
const SUBAGENT_FAILED_STATUSES = new Set(['failed', 'error', 'timeout', 'partial', 'not_found']);
const SUBAGENT_CANCELLED_STATUSES = new Set(['cancelled', 'closed']);

const HEX_DIRECTIONS: HoneycombSlot[] = [
  { q: 1, r: 0 },
  { q: 1, r: -1 },
  { q: 0, r: -1 },
  { q: -1, r: 0 },
  { q: -1, r: 1 },
  { q: 0, r: 1 }
];

const CARD_ACCENT_PALETTE = ['#3b82f6', '#8b5cf6', '#22c55e', '#06b6d4', '#eab308', '#f97316', '#ef4444'];

const trimText = (value: unknown, max: number) => {
  const text = String(value || '').trim();
  if (!text) return '';
  if (text.length <= max) return text;
  return `${text.slice(0, max)}...`;
};

const hashText = (value: string) => {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash << 5) - hash + value.charCodeAt(index);
    hash |= 0;
  }
  return Math.abs(hash);
};

const resolveNodeAccent = (
  agentId: string,
  role: SwarmNodeRole,
  options: { emphasis?: SwarmNodeEmphasis; failed?: boolean } = {}
) => {
  if (role === 'mother') return '#f59e0b';
  if (role === 'subagent') {
    if (options.failed) return '#fb7185';
    if (options.emphasis === 'dormant') return '#64748b';
    if (options.emphasis === 'active') return '#22d3ee';
    return '#38bdf8';
  }
  return CARD_ACCENT_PALETTE[hashText(agentId) % CARD_ACCENT_PALETTE.length] || '#3b82f6';
};

export const resolveBeeroomSwarmScopeKey = (options: {
  missionId?: unknown;
  teamRunId?: unknown;
  groupId?: unknown;
  fallback?: unknown;
}) => {
  const key = String(options.missionId || options.teamRunId || options.groupId || options.fallback || 'standby').trim();
  return key || 'standby';
};

export const resolveBeeroomMotherAgentId = (
  mission: BeeroomMission | null | undefined,
  group: BeeroomGroup | null | undefined,
  agents: BeeroomMember[]
) =>
  String(mission?.mother_agent_id || group?.mother_agent_id || mission?.entry_agent_id || agents[0]?.agent_id || '').trim();

const mergeProjectionMembers = (
  group: BeeroomGroup | null | undefined,
  agents: BeeroomMember[],
  dispatchPreview: BeeroomSwarmDispatchPreview | null | undefined
): BeeroomMember[] => {
  const merged = new Map<string, BeeroomMember>();
  const pushMember = (member: BeeroomMember | null | undefined) => {
    const agentId = String(member?.agent_id || '').trim();
    if (!agentId) return;
    const current = merged.get(agentId) || ({} as BeeroomMember);
    merged.set(agentId, {
      ...current,
      ...member,
      agent_id: agentId
    });
  };

  (Array.isArray(group?.members) ? group?.members : []).forEach(pushMember);
  (Array.isArray(agents) ? agents : []).forEach(pushMember);

  const motherAgentId = String(group?.mother_agent_id || '').trim();
  if (motherAgentId && !merged.has(motherAgentId)) {
    pushMember({
      agent_id: motherAgentId,
      name: String(group?.mother_agent_name || motherAgentId).trim() || motherAgentId,
      idle: true,
      active_session_total: 0
    });
  }

  const dispatchAgentId = String(dispatchPreview?.targetAgentId || '').trim();
  if (dispatchAgentId && !merged.has(dispatchAgentId)) {
    const active = ACTIVE_DISPATCH_STATUSES.has(String(dispatchPreview?.status || '').trim().toLowerCase());
    pushMember({
      agent_id: dispatchAgentId,
      name: String(dispatchPreview?.targetName || dispatchAgentId).trim() || dispatchAgentId,
      idle: !active,
      active_session_total: active ? 1 : 0
    });
  }

  return Array.from(merged.values());
};

const resolveNodeStatus = (tasks: BeeroomMissionTask[], member: BeeroomMember | undefined, missionStatus: string) => {
  if (member?.idle === false) return 'running';
  if (!tasks.length) return missionStatus || 'idle';
  const statuses = tasks.map((task) => String(task.status || '').trim().toLowerCase());
  if (statuses.some((status) => status === 'running' || status === 'queued')) return 'running';
  if (statuses.some((status) => status === 'failed' || status === 'error' || status === 'timeout')) return 'failed';
  if (statuses.some((status) => status === 'cancelled')) return 'cancelled';
  if (statuses.every((status) => status === 'success' || status === 'completed')) return 'completed';
  return missionStatus || 'idle';
};

const resolveStatusRank = (status: string) => {
  const normalized = String(status || '').trim().toLowerCase();
  const rankMap: Record<string, number> = {
    running: 0,
    queued: 1,
    awaiting_idle: 2,
    failed: 3,
    cancelled: 4,
    completed: 5,
    success: 5,
    idle: 6
  };
  return rankMap[normalized] ?? 7;
};

const pickLatestTask = (tasks: BeeroomMissionTask[]) =>
  [...tasks].sort(compareBeeroomMissionTasksByDisplayPriority)[0] || null;

const buildHoneycombSlots = (count: number): HoneycombSlot[] => {
  if (count <= 0) return [];
  const slots: HoneycombSlot[] = [{ q: 0, r: 0 }];
  let ring = 1;
  while (slots.length < count) {
    let current: HoneycombSlot = { q: -ring, r: ring };
    for (const direction of HEX_DIRECTIONS) {
      for (let step = 0; step < ring; step += 1) {
        if (slots.length >= count) break;
        slots.push({ ...current });
        current = {
          q: current.q + direction.q,
          r: current.r + direction.r
        };
      }
    }
    ring += 1;
  }
  return slots.slice(0, count);
};

const resolveHoneycombPosition = (slot: HoneycombSlot) => ({
  x: Math.round(HONEYCOMB_RADIUS * Math.sqrt(3) * (slot.q + slot.r / 2)),
  y: Math.round(HONEYCOMB_RADIUS * HONEYCOMB_VERTICAL_RATIO * slot.r)
});

const resolveStatusLabel = (value: unknown, t: TranslationFn) => {
  const normalized = String(value || '').trim().toLowerCase();
  const keyMap: Record<string, string> = {
    accepted: 'beeroom.status.queued',
    queued: 'beeroom.status.queued',
    waiting: 'beeroom.status.running',
    running: 'beeroom.status.running',
    cancelling: 'beeroom.status.running',
    awaiting_idle: 'beeroom.status.awaitingIdle',
    completed: 'beeroom.status.completed',
    success: 'beeroom.status.completed',
    failed: 'beeroom.status.failed',
    error: 'beeroom.status.failed',
    partial: 'beeroom.status.failed',
    not_found: 'beeroom.status.failed',
    timeout: 'beeroom.status.timeout',
    cancelled: 'beeroom.status.cancelled',
    closed: 'beeroom.status.cancelled',
    idle: 'beeroom.members.idle'
  };
  return t(keyMap[normalized] || 'beeroom.status.unknown');
};

const trimDispatchLabel = (value: unknown, max = 26) => {
  const text = String(value || '').replace(/\s+/g, ' ').trim();
  if (!text) return '';
  if (text.length <= max) return text;
  return `${text.slice(0, max)}...`;
};

const resolveDispatchTaskLabel = (mission: BeeroomMission | null, task: BeeroomMissionTask | null) => {
  const missionText = trimDispatchLabel(mission?.summary || mission?.strategy || '');
  if (missionText) return missionText;
  const taskText = trimDispatchLabel(task?.result_summary || task?.error || '');
  if (taskText) return taskText;
  const taskId = String(task?.task_id || '').trim();
  return taskId ? `#${taskId.slice(0, 8)}` : '';
};

const normalizeDispatchPreviewStatus = (value: unknown): string => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'queued') return 'queued';
  if (normalized === 'awaiting_approval' || normalized === 'resuming' || normalized === 'running') {
    return 'running';
  }
  if (normalized === 'completed' || normalized === 'success') return 'completed';
  if (normalized === 'failed' || normalized === 'error') return 'failed';
  if (normalized === 'stopped' || normalized === 'cancelled') return 'cancelled';
  return normalized || 'idle';
};

const resolveDispatchPreviewTone = (value: unknown): BeeroomWorkflowTone => {
  const normalized = normalizeDispatchPreviewStatus(value);
  if (normalized === 'failed' || normalized === 'cancelled') return 'failed';
  if (normalized === 'queued') return 'pending';
  if (normalized === 'running') return 'loading';
  return 'completed';
};

const buildDispatchPreviewLines = (
  preview: BeeroomSwarmDispatchPreview,
  statusLabel: string,
  t: TranslationFn
): BeeroomNodeWorkflowLine[] => {
  const lines: BeeroomNodeWorkflowLine[] = [];
  const sessionId = String(preview.sessionId || '').trim();
  const label = trimText(preview.dispatchLabel || preview.summary || preview.targetName, 30);
  lines.push({
    key: `${sessionId}:dispatch`,
    main: statusLabel,
    detail: label || '-',
    title: preview.dispatchLabel || preview.summary || preview.targetName || statusLabel
  });
  if (sessionId) {
    lines.push({
      key: `${sessionId}:session`,
      main: t('beeroom.task.sessionId'),
      detail: trimText(sessionId, 30),
      title: sessionId
    });
  }
  if (preview.summary) {
    lines.push({
      key: `${sessionId}:summary`,
      main: t('beeroom.canvas.subagentSummary'),
      detail: trimText(preview.summary, 30),
      title: preview.summary
    });
  }
  return lines.slice(0, 3);
};

const resolveAgentAvatarImage = (icon: unknown): string =>
  resolveAgentAvatarImageByConfig(parseAgentAvatarIconConfig(icon));

const buildWorkflowSnapshot = (
  task: BeeroomMissionTask | null,
  itemsByTask: Record<string, BeeroomWorkflowItem[]>,
  previewByTask: Record<string, BeeroomTaskWorkflowPreview>,
  t: TranslationFn
) => {
  if (!task) {
    return buildTaskWorkflowRuntime(null, [], t);
  }
  const taskId = String(task.task_id || '').trim();
  const items = taskId ? itemsByTask[taskId] : null;
  if (Array.isArray(items) && items.length) {
    return {
      items,
      preview: previewByTask[taskId] || buildTaskWorkflowRuntime(task, [], t).preview
    };
  }
  return buildTaskWorkflowRuntime(task, [], t);
};

const buildSubagentSummaryLines = (
  item: BeeroomMissionSubagentItem,
  visualState: SubagentVisualState,
  t: TranslationFn
): BeeroomNodeWorkflowLine[] => {
  const lines: BeeroomNodeWorkflowLine[] = [];
  const modeParts = [item.spawnMode, item.strategy, item.controlScope].filter(Boolean);
  if (modeParts.length > 0) {
    const detail = trimText(modeParts.join(' · '), 28);
    lines.push({
      key: `${item.key}:mode`,
      main: trimText(item.dispatchLabel || item.label || item.title, 14) || '-',
      detail,
      title: modeParts.join(' · ')
    });
  }
  const identityParts = [
    item.depth !== null && item.depth >= 0 ? `D${item.depth}` : '',
    item.runId ? trimText(item.runId, 16) : '',
    item.sessionId ? trimText(item.sessionId, 16) : ''
  ].filter(Boolean);
  if (identityParts.length > 0) {
    const detail = identityParts.join(' · ');
    lines.push({
      key: `${item.key}:identity`,
      main: visualState.statusLabel,
      detail: trimText(detail, 30),
      title: detail
    });
  }
  if (item.summary) {
    lines.push({
      key: `${item.key}:summary`,
      main: t('beeroom.canvas.subagentSummary'),
      detail: trimText(item.summary, 30),
      title: item.summary
    });
  }
  return lines.slice(0, 3);
};

const resolveSubagentVisualState = (item: BeeroomMissionSubagentItem, t: TranslationFn): SubagentVisualState => {
  const normalizedStatus = String(item.status || '').trim().toLowerCase();
  if (SUBAGENT_ACTIVE_STATUSES.has(normalizedStatus)) {
    const status = normalizedStatus === 'accepted' ? 'queued' : normalizedStatus === 'waiting' ? 'running' : normalizedStatus;
    return {
      status,
      statusLabel: resolveStatusLabel(status, t),
      emphasis: 'active',
      workflowTone: 'loading',
      accentColor: resolveNodeAccent(item.agentId || item.sessionId || item.key, 'subagent', { emphasis: 'active' }),
      active: true
    };
  }
  if (SUBAGENT_FAILED_STATUSES.has(normalizedStatus)) {
    return {
      status: normalizedStatus === 'partial' || normalizedStatus === 'not_found' ? 'failed' : normalizedStatus,
      statusLabel: resolveStatusLabel(normalizedStatus, t),
      emphasis: 'dormant',
      workflowTone: 'failed',
      accentColor: resolveNodeAccent(item.agentId || item.sessionId || item.key, 'subagent', {
        emphasis: 'dormant',
        failed: true
      }),
      active: false
    };
  }
  if (SUBAGENT_CANCELLED_STATUSES.has(normalizedStatus)) {
    return {
      status: 'cancelled',
      statusLabel: resolveStatusLabel(normalizedStatus, t),
      emphasis: 'dormant',
      workflowTone: 'failed',
      accentColor: resolveNodeAccent(item.agentId || item.sessionId || item.key, 'subagent', { emphasis: 'dormant' }),
      active: false
    };
  }
  return {
    status: 'completed',
    statusLabel: resolveStatusLabel('completed', t),
    emphasis: 'dormant',
    workflowTone: 'completed',
    accentColor: resolveNodeAccent(item.agentId || item.sessionId || item.key, 'subagent', { emphasis: 'dormant' }),
    active: false
  };
};

const resolveSubagentBranchVector = (
  motherNode: Pick<SwarmProjectionNode, 'x' | 'y'> | null,
  workerNode: Pick<SwarmProjectionNode, 'x' | 'y'>,
  fallbackSeed: number
): SubagentBranchVector => {
  const dx = workerNode.x - Number(motherNode?.x || 0);
  const dy = workerNode.y - Number(motherNode?.y || 0);
  const length = Math.hypot(dx, dy);
  if (length > 1) {
    const axisX = dx / length;
    const axisY = dy / length;
    return {
      axisX,
      axisY,
      perpX: -axisY,
      perpY: axisX
    };
  }
  const angle = (fallbackSeed % 12) * (Math.PI / 6);
  const axisX = Math.cos(angle);
  const axisY = Math.sin(angle);
  return {
    axisX,
    axisY,
    perpX: -axisY,
    perpY: axisX
  };
};

const resolveSubagentBranchPosition = (
  workerNode: Pick<SwarmProjectionNode, 'x' | 'y'>,
  motherNode: Pick<SwarmProjectionNode, 'x' | 'y'> | null,
  index: number,
  total: number,
  fallbackSeed: number
) => {
  const vector = resolveSubagentBranchVector(motherNode, workerNode, fallbackSeed);
  const row = Math.floor(index / SUBAGENT_ROW_CAPACITY);
  const indexInRow = index % SUBAGENT_ROW_CAPACITY;
  const rowCount = Math.min(SUBAGENT_ROW_CAPACITY, total - row * SUBAGENT_ROW_CAPACITY);
  const lane = indexInRow - (rowCount - 1) / 2;
  const forward = SUBAGENT_BRANCH_BASE_GAP + row * SUBAGENT_BRANCH_ROW_GAP;
  const lateral = lane * SUBAGENT_BRANCH_LANE_GAP;
  return {
    x: Math.round(workerNode.x + vector.axisX * forward + vector.perpX * lateral),
    y: Math.round(workerNode.y + vector.axisY * forward + vector.perpY * lateral)
  };
};

export const hasBeeroomSwarmNodes = (options: {
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
  dispatchPreview?: BeeroomSwarmDispatchPreview | null;
}) => {
  const mission = options.mission;
  const members = mergeProjectionMembers(options.group, options.agents, options.dispatchPreview || null);
  const activeAgentIds = new Set<string>();
  const involvedAgentIds = new Set<string>();
  members.forEach((member) => {
    const agentId = String(member.agent_id || '').trim();
    if (!agentId) return;
    activeAgentIds.add(agentId);
    involvedAgentIds.add(agentId);
  });
  (Array.isArray(mission?.tasks) ? mission.tasks : []).forEach((task) => {
    const agentId = String(task.agent_id || '').trim();
    if (agentId && activeAgentIds.has(agentId)) involvedAgentIds.add(agentId);
  });
  const motherAgentId = resolveBeeroomMotherAgentId(mission, options.group, members);
  if (motherAgentId && activeAgentIds.has(motherAgentId)) involvedAgentIds.add(motherAgentId);
  const entryAgentId = String(mission?.entry_agent_id || '').trim();
  if (entryAgentId && activeAgentIds.has(entryAgentId)) {
    involvedAgentIds.add(entryAgentId);
  }
  const dispatchAgentId = String(options.dispatchPreview?.targetAgentId || '').trim();
  if (dispatchAgentId) {
    involvedAgentIds.add(dispatchAgentId);
  }
  if (options.dispatchPreview?.subagents?.length) {
    return true;
  }
  return involvedAgentIds.size > 0;
};

export const buildBeeroomSwarmProjection = (options: {
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
  selectedNodeId: string;
  nodePositionOverrides: Record<string, BeeroomCanvasPositionOverride>;
  dispatchPreview: BeeroomSwarmDispatchPreview | null;
  subagentsByTask: Record<string, BeeroomMissionSubagentItem[]>;
  workflowItemsByTask: Record<string, BeeroomWorkflowItem[]>;
  workflowPreviewByTask: Record<string, BeeroomTaskWorkflowPreview>;
  t: TranslationFn;
}): SwarmProjection => {
  const mission = options.mission;
  const tasks = Array.isArray(mission?.tasks) ? mission.tasks : [];
  const members = mergeProjectionMembers(options.group, options.agents, options.dispatchPreview || null);
  const memberMap = new Map(members.map((agent) => [String(agent.agent_id || '').trim(), agent]));
  const activeAgentIds = new Set(Array.from(memberMap.keys()).filter(Boolean));
  const motherAgentId = resolveBeeroomMotherAgentId(mission, options.group, members);
  const hasMotherAgent = Boolean(motherAgentId) && activeAgentIds.has(motherAgentId);
  const entryAgentId = String(mission?.entry_agent_id || '').trim();
  const hasEntryAgent = Boolean(entryAgentId) && activeAgentIds.has(entryAgentId);
  const missionStatus = String(mission?.completion_status || mission?.status || '').trim().toLowerCase() || 'idle';
  const involvedAgentIds = new Set<string>();

  members.forEach((member) => {
    const agentId = String(member.agent_id || '').trim();
    if (agentId) involvedAgentIds.add(agentId);
  });
  tasks.forEach((task) => {
    const agentId = String(task.agent_id || '').trim();
    if (agentId && activeAgentIds.has(agentId)) involvedAgentIds.add(agentId);
  });
  if (hasMotherAgent) involvedAgentIds.add(motherAgentId);
  if (hasEntryAgent) involvedAgentIds.add(entryAgentId);

  if (!involvedAgentIds.size) {
    return {
      nodes: [],
      edges: [],
      nodeMetaMap: new Map<string, CanvasNodeMeta>(),
      memberMap,
      tasksByAgent: new Map<string, BeeroomMissionTask[]>(),
      motherNodeId: '',
      bounds: { minX: 0, minY: 0, maxX: 0, maxY: 0, width: 0, height: 0 }
    };
  }

  const tasksByAgent = new Map<string, BeeroomMissionTask[]>();
  tasks.forEach((task) => {
    const agentId = String(task.agent_id || '').trim();
    if (!agentId || !activeAgentIds.has(agentId)) return;
    const bucket = tasksByAgent.get(agentId) || [];
    bucket.push(task);
    tasksByAgent.set(agentId, bucket);
  });

  const orderedAgentIds = Array.from(involvedAgentIds).sort((left, right) => {
    if (left === motherAgentId) return -1;
    if (right === motherAgentId) return 1;
    if (left === entryAgentId) return -1;
    if (right === entryAgentId) return 1;
    const leftTasks = tasksByAgent.get(left) || [];
    const rightTasks = tasksByAgent.get(right) || [];
    const leftStatus = resolveNodeStatus(leftTasks, memberMap.get(left), missionStatus);
    const rightStatus = resolveNodeStatus(rightTasks, memberMap.get(right), missionStatus);
    const rankDiff = resolveStatusRank(leftStatus) - resolveStatusRank(rightStatus);
    if (rankDiff !== 0) return rankDiff;
    const taskDiff = rightTasks.length - leftTasks.length;
    if (taskDiff !== 0) return taskDiff;
    return String(memberMap.get(left)?.name || left).localeCompare(String(memberMap.get(right)?.name || right), 'zh-Hans-CN');
  });

  const slots = buildHoneycombSlots(orderedAgentIds.length);
  const latestTaskByAgent = new Map<string, BeeroomMissionTask | null>();
  const nodeMetaMap = new Map<string, CanvasNodeMeta>();
  let minX = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;

  const includeBounds = (x: number, y: number, width: number, height: number) => {
    minX = Math.min(minX, x - width / 2);
    maxX = Math.max(maxX, x + width / 2);
    minY = Math.min(minY, y - height / 2);
    maxY = Math.max(maxY, y + height / 2);
  };

  const workerNodes = orderedAgentIds.map((agentId, index) => {
    const member = memberMap.get(agentId);
    const agentTasks = tasksByAgent.get(agentId) || [];
    const latestTask = pickLatestTask(agentTasks);
    latestTaskByAgent.set(agentId, latestTask);
    const isMother = (hasMotherAgent && agentId === motherAgentId) || (!hasMotherAgent && index === 0);
    const status = resolveNodeStatus(agentTasks, member, missionStatus);
    const statusLabel = resolveStatusLabel(status, options.t);
    const nodeId = `agent:${agentId}`;
    const roleLabel = isMother ? options.t('beeroom.canvas.legendMother') : options.t('beeroom.canvas.legendWorker');
    const name = String(member?.name || (isMother ? options.group?.mother_agent_name : '') || agentId).trim();
    const summary = String(
      agentTasks.map((task) => task.result_summary || task.error || '').find((item) => String(item || '').trim()) ||
        member?.description ||
        ''
    ).trim();
    const slot = slots[index] || { q: 0, r: 0 };
    const position = options.nodePositionOverrides[nodeId] || resolveHoneycombPosition(slot);
    const workflowSnapshot = buildWorkflowSnapshot(
      latestTask,
      options.workflowItemsByTask,
      options.workflowPreviewByTask,
      options.t
    );
    const workflowLines = buildNodeWorkflowPreviewLines(workflowSnapshot.items);
    const meta: CanvasNodeMeta = {
      id: nodeId,
      agent_id: agentId,
      agent_name: name,
      role: isMother ? 'mother' : 'worker',
      role_label: roleLabel,
      status,
      task_total: agentTasks.length,
      active_session_total: Number(member?.active_session_total || 0),
      updated_time: Math.max(...agentTasks.map((task) => resolveBeeroomTaskMoment(task)), Number(mission?.updated_time || 0)),
      summary,
      entry_agent: agentId === entryAgentId,
      parent_id: '',
      emphasis: 'default'
    };
    nodeMetaMap.set(nodeId, meta);
    includeBounds(position.x, position.y, NODE_WIDTH, NODE_HEIGHT);

    return {
      id: nodeId,
      agentId,
      name,
      displayName: trimText(name, 12) || '-',
      role: meta.role,
      roleLabel,
      status,
      statusLabel,
      selected: nodeId === String(options.selectedNodeId || '').trim(),
      accentColor: resolveNodeAccent(agentId, meta.role),
      avatarInitial: resolveAgentAvatarInitial(name),
      avatarImageUrl: resolveAgentAvatarImage(member?.icon),
      x: position.x,
      y: position.y,
      width: NODE_WIDTH,
      height: NODE_HEIGHT,
      workflowTaskId: String(latestTask?.task_id || '').trim(),
      workflowTone: workflowSnapshot.preview.badgeTone,
      workflowLines,
      parentId: '',
      emphasis: 'default',
      introFromId: '',
      introOrder: 0
    } satisfies SwarmProjectionNode;
  });

  if (!workerNodes.length) {
    return {
      nodes: [],
      edges: [],
      nodeMetaMap,
      memberMap,
      tasksByAgent,
      motherNodeId: '',
      bounds: { minX: 0, minY: 0, maxX: 0, maxY: 0, width: 0, height: 0 }
    };
  }

  const effectiveMotherAgentId = (hasMotherAgent ? motherAgentId : '') || orderedAgentIds[0] || '';
  const motherNodeId = effectiveMotherAgentId ? `agent:${effectiveMotherAgentId}` : workerNodes[0]?.id || '';
  const motherNode = workerNodes.find((node) => node.id === motherNodeId) || workerNodes[0] || null;
  const runtimeDispatch = !tasks.length ? options.dispatchPreview || null : null;
  const edges: SwarmProjectionEdge[] = [];

  if (effectiveMotherAgentId) {
    orderedAgentIds.forEach((agentId) => {
      if (!agentId || agentId === effectiveMotherAgentId) return;
      const agentTasks = tasksByAgent.get(agentId) || [];
      const latestTask = latestTaskByAgent.get(agentId) || pickLatestTask(agentTasks);
      const latestStatus = String(latestTask?.status || '').trim().toLowerCase();
      const dispatchActive =
        ACTIVE_DISPATCH_STATUSES.has(latestStatus) || (memberMap.get(agentId)?.idle === false && agentTasks.length > 0);
      const targetNodeId = `agent:${agentId}`;
      const edgeSelected =
        Boolean(options.selectedNodeId) &&
        (options.selectedNodeId === motherNodeId || options.selectedNodeId === targetNodeId);
      edges.push({
        id: `dispatch:${effectiveMotherAgentId}:${agentId}`,
        source: motherNodeId,
        target: targetNodeId,
        label: dispatchActive ? resolveDispatchTaskLabel(mission, latestTask) : '',
        active: dispatchActive,
        selected: edgeSelected,
        kind: 'dispatch'
      });
    });
  }

  const runtimeTargetAgentId = String(runtimeDispatch?.targetAgentId || '').trim();
  const runtimeTargetNode =
    (runtimeTargetAgentId
      ? workerNodes.find((node) => node.agentId === runtimeTargetAgentId)
      : null) ||
    motherNode ||
    workerNodes[0] ||
    null;

  if (runtimeDispatch && runtimeTargetNode) {
    const previewStatus = normalizeDispatchPreviewStatus(runtimeDispatch.status) || runtimeTargetNode.status;
    const statusLabel = resolveStatusLabel(previewStatus, options.t);
    runtimeTargetNode.status = previewStatus;
    runtimeTargetNode.statusLabel = statusLabel;
    runtimeTargetNode.workflowTone = resolveDispatchPreviewTone(previewStatus);
    runtimeTargetNode.workflowLines = buildDispatchPreviewLines(runtimeDispatch, statusLabel, options.t);
    const targetMeta = nodeMetaMap.get(runtimeTargetNode.id);
    if (targetMeta) {
      targetMeta.status = previewStatus;
      targetMeta.updated_time = Math.max(targetMeta.updated_time, Number(runtimeDispatch.updatedTime || 0));
      if (runtimeDispatch.summary) {
        targetMeta.summary = runtimeDispatch.summary;
      }
    }
    if (motherNode && motherNode.id !== runtimeTargetNode.id) {
      const previewActive = previewStatus === 'queued' || previewStatus === 'running';
      edges.push({
        id: `dispatch:runtime:${motherNode.id}:${runtimeTargetNode.id}:${runtimeDispatch.sessionId}`,
        source: motherNode.id,
        target: runtimeTargetNode.id,
        label: previewActive ? trimDispatchLabel(runtimeDispatch.dispatchLabel || runtimeDispatch.summary, 18) : '',
        active: previewActive,
        selected:
          Boolean(options.selectedNodeId) &&
          (options.selectedNodeId === motherNode.id || options.selectedNodeId === runtimeTargetNode.id),
        kind: 'dispatch'
      });
    }
  }

  const subagentNodes: SwarmProjectionNode[] = [];

  workerNodes.forEach((workerNode, workerIndex) => {
    const latestTask = latestTaskByAgent.get(workerNode.agentId);
    const runtimeSubagents =
      runtimeDispatch && runtimeTargetNode?.id === workerNode.id
        ? runtimeDispatch.subagents
        : [];
    const taskId = String(latestTask?.task_id || '').trim();
    const taskSubagents =
      taskId && workerNode.role !== 'mother'
        ? Array.isArray(options.subagentsByTask[taskId])
          ? options.subagentsByTask[taskId]
          : []
        : [];
    const subagents = runtimeSubagents.length > 0 ? runtimeSubagents : taskSubagents;
    if (!subagents.length) return;
    subagents.forEach((item, subagentIndex) => {
      const nodeId = `subagent:${item.sessionId || item.runId || item.key}`;
      const visualState = resolveSubagentVisualState(item, options.t);
      const name =
        String(
          item.label ||
            item.title ||
            memberMap.get(item.agentId)?.name ||
            item.agentId ||
            item.sessionId ||
            item.runId ||
            item.key
        ).trim() || '-';
      const position =
        options.nodePositionOverrides[nodeId] ||
        resolveSubagentBranchPosition(workerNode, motherNode, subagentIndex, subagents.length, workerIndex + 1);
      const workflowLines = buildSubagentSummaryLines(item, visualState, options.t);
      const meta: CanvasNodeMeta = {
        id: nodeId,
        agent_id: item.agentId,
        agent_name: name,
        role: 'subagent',
        role_label: options.t('beeroom.canvas.legendSubagent'),
        status: visualState.status,
        task_total: 0,
        active_session_total: 1,
        updated_time: Number(item.updatedTime || 0),
        summary: item.summary,
        entry_agent: false,
        parent_id: workerNode.id,
        emphasis: visualState.emphasis
      };
      nodeMetaMap.set(nodeId, meta);
      includeBounds(position.x, position.y, SUBAGENT_NODE_WIDTH, SUBAGENT_NODE_HEIGHT);
      subagentNodes.push({
        id: nodeId,
        agentId: item.agentId,
        name,
        displayName: trimText(name, 12) || '-',
        role: 'subagent',
        roleLabel: options.t('beeroom.canvas.legendSubagent'),
        status: visualState.status,
        statusLabel: visualState.statusLabel,
        selected: nodeId === String(options.selectedNodeId || '').trim(),
        accentColor: visualState.accentColor,
        avatarInitial: resolveAgentAvatarInitial(name),
        avatarImageUrl: resolveAgentAvatarImage(memberMap.get(item.agentId)?.icon),
        x: position.x,
        y: position.y,
        width: SUBAGENT_NODE_WIDTH,
        height: SUBAGENT_NODE_HEIGHT,
        workflowTaskId: taskId,
        workflowTone: visualState.workflowTone,
        workflowLines,
        parentId: workerNode.id,
        emphasis: visualState.emphasis,
        introFromId: workerNode.id,
        introOrder: subagentIndex
      } satisfies SwarmProjectionNode);
      edges.push({
        id: `subagent:${workerNode.id}:${item.sessionId || item.runId || item.key}`,
        source: workerNode.id,
        target: nodeId,
        label: visualState.active ? trimDispatchLabel(item.dispatchLabel || item.label || item.title, 18) : '',
        active: visualState.active,
        selected:
          Boolean(options.selectedNodeId) &&
          (options.selectedNodeId === workerNode.id || options.selectedNodeId === nodeId),
        kind: 'subagent'
      });
    });
  });

  const nodes = [...workerNodes, ...subagentNodes];
  const bounds = {
    minX: Number.isFinite(minX) ? minX : 0,
    minY: Number.isFinite(minY) ? minY : 0,
    maxX: Number.isFinite(maxX) ? maxX : 0,
    maxY: Number.isFinite(maxY) ? maxY : 0,
    width: Math.max(0, (Number.isFinite(maxX) ? maxX : 0) - (Number.isFinite(minX) ? minX : 0)),
    height: Math.max(0, (Number.isFinite(maxY) ? maxY : 0) - (Number.isFinite(minY) ? minY : 0))
  };

  return {
    nodes,
    edges,
    nodeMetaMap,
    memberMap,
    tasksByAgent,
    motherNodeId: motherNodeId || nodes[0]?.id || '',
    bounds
  };
};
