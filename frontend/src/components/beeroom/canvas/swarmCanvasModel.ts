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
import type {
  BeeroomCanvasPositionOverride
} from '@/components/beeroom/beeroomMissionCanvasStateCache';
import type {
  BeeroomGroup,
  BeeroomMember,
  BeeroomMission,
  BeeroomMissionTask
} from '@/stores/beeroom';
import {
  parseAgentAvatarIconConfig,
  resolveAgentAvatarImageByConfig,
  resolveAgentAvatarInitial
} from '@/utils/agentAvatar';

export type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

export type CanvasNodeMeta = {
  id: string;
  agent_id: string;
  agent_name: string;
  role: 'mother' | 'worker';
  role_label: string;
  status: string;
  task_total: number;
  active_session_total: number;
  updated_time: number;
  summary: string;
  entry_agent: boolean;
};

export type SwarmProjectionNode = {
  id: string;
  agentId: string;
  name: string;
  displayName: string;
  role: 'mother' | 'worker';
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
};

export type SwarmProjectionEdge = {
  id: string;
  source: string;
  target: string;
  label: string;
  active: boolean;
  selected: boolean;
  kind: 'dispatch';
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

type HoneycombSlot = {
  q: number;
  r: number;
};

export const HONEYCOMB_RADIUS = 186;
export const HONEYCOMB_VERTICAL_RATIO = 1.2;
export const NODE_WIDTH = 272;
export const NODE_HEIGHT = 138;
export const WORLD_PADDING = 96;
export const ACTIVE_DISPATCH_STATUSES = new Set(['queued', 'running', 'awaiting_idle']);

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

const resolveNodeAccent = (agentId: string, isMother: boolean) => {
  if (isMother) return '#f59e0b';
  return CARD_ACCENT_PALETTE[hashText(agentId) % CARD_ACCENT_PALETTE.length] || '#3b82f6';
};

export const resolveBeeroomSwarmScopeKey = (options: {
  missionId?: unknown;
  teamRunId?: unknown;
  groupId?: unknown;
  fallback?: unknown;
}) => {
  const key = String(
    options.missionId || options.teamRunId || options.groupId || options.fallback || 'standby'
  ).trim();
  return key || 'standby';
};

export const resolveBeeroomMotherAgentId = (
  mission: BeeroomMission | null | undefined,
  group: BeeroomGroup | null | undefined,
  agents: BeeroomMember[]
) =>
  String(
    mission?.mother_agent_id ||
      group?.mother_agent_id ||
      mission?.entry_agent_id ||
      agents[0]?.agent_id ||
      ''
  ).trim();

const resolveNodeStatus = (
  tasks: BeeroomMissionTask[],
  member: BeeroomMember | undefined,
  missionStatus: string
) => {
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
    queued: 'beeroom.status.queued',
    running: 'beeroom.status.running',
    awaiting_idle: 'beeroom.status.awaitingIdle',
    completed: 'beeroom.status.completed',
    success: 'beeroom.status.completed',
    failed: 'beeroom.status.failed',
    error: 'beeroom.status.failed',
    timeout: 'beeroom.status.timeout',
    cancelled: 'beeroom.status.cancelled',
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

export const hasBeeroomSwarmNodes = (options: {
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
}) => {
  const mission = options.mission;
  const involvedAgentIds = new Set<string>();
  options.agents.forEach((member) => {
    const agentId = String(member.agent_id || '').trim();
    if (agentId) involvedAgentIds.add(agentId);
  });
  (Array.isArray(mission?.tasks) ? mission.tasks : []).forEach((task) => {
    const agentId = String(task.agent_id || '').trim();
    if (agentId) involvedAgentIds.add(agentId);
  });
  const motherAgentId = resolveBeeroomMotherAgentId(mission, options.group, options.agents);
  if (motherAgentId) involvedAgentIds.add(motherAgentId);
  if (String(mission?.entry_agent_id || '').trim()) {
    involvedAgentIds.add(String(mission?.entry_agent_id || '').trim());
  }
  return involvedAgentIds.size > 0;
};

export const buildBeeroomSwarmProjection = (options: {
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
  selectedNodeId: string;
  nodePositionOverrides: Record<string, BeeroomCanvasPositionOverride>;
  workflowItemsByTask: Record<string, BeeroomWorkflowItem[]>;
  workflowPreviewByTask: Record<string, BeeroomTaskWorkflowPreview>;
  t: TranslationFn;
}): SwarmProjection => {
  const mission = options.mission;
  const tasks = Array.isArray(mission?.tasks) ? mission.tasks : [];
  const members = Array.isArray(options.agents) ? options.agents : [];
  const memberMap = new Map(members.map((agent) => [String(agent.agent_id || '').trim(), agent]));
  const motherAgentId = resolveBeeroomMotherAgentId(mission, options.group, members);
  const entryAgentId = String(mission?.entry_agent_id || '').trim();
  const missionStatus =
    String(mission?.completion_status || mission?.status || '').trim().toLowerCase() || 'idle';
  const involvedAgentIds = new Set<string>();

  members.forEach((member) => {
    const agentId = String(member.agent_id || '').trim();
    if (agentId) involvedAgentIds.add(agentId);
  });
  tasks.forEach((task) => {
    const agentId = String(task.agent_id || '').trim();
    if (agentId) involvedAgentIds.add(agentId);
  });
  if (motherAgentId) involvedAgentIds.add(motherAgentId);
  if (entryAgentId) involvedAgentIds.add(entryAgentId);

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
    if (!agentId) return;
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
    return String(memberMap.get(left)?.name || left).localeCompare(
      String(memberMap.get(right)?.name || right),
      'zh-Hans-CN'
    );
  });

  const slots = buildHoneycombSlots(orderedAgentIds.length);
  const nodeMetaMap = new Map<string, CanvasNodeMeta>();
  let minX = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;

  const nodes = orderedAgentIds.map((agentId, index) => {
    const member = memberMap.get(agentId);
    const agentTasks = tasksByAgent.get(agentId) || [];
    const latestTask = pickLatestTask(agentTasks);
    const isMother = agentId === motherAgentId || (!motherAgentId && index === 0);
    const status = resolveNodeStatus(agentTasks, member, missionStatus);
    const statusLabel = resolveStatusLabel(status, options.t);
    const nodeId = `agent:${agentId}`;
    const roleLabel = isMother ? options.t('beeroom.canvas.legendMother') : options.t('beeroom.canvas.legendWorker');
    const name = String(member?.name || (isMother ? options.group?.mother_agent_name : '') || agentId).trim();
    const summary = String(
      agentTasks
        .map((task) => task.result_summary || task.error || '')
        .find((item) => String(item || '').trim()) || member?.description || ''
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
      updated_time: Math.max(
        ...agentTasks.map((task) => resolveBeeroomTaskMoment(task)),
        Number(mission?.updated_time || 0)
      ),
      summary,
      entry_agent: agentId === entryAgentId
    };
    nodeMetaMap.set(nodeId, meta);

    minX = Math.min(minX, position.x - NODE_WIDTH / 2);
    maxX = Math.max(maxX, position.x + NODE_WIDTH / 2);
    minY = Math.min(minY, position.y - NODE_HEIGHT / 2);
    maxY = Math.max(maxY, position.y + NODE_HEIGHT / 2);

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
      accentColor: resolveNodeAccent(agentId, isMother),
      avatarInitial: resolveAgentAvatarInitial(name),
      avatarImageUrl: resolveAgentAvatarImage(member?.icon),
      x: position.x,
      y: position.y,
      width: NODE_WIDTH,
      height: NODE_HEIGHT,
      workflowTaskId: String(latestTask?.task_id || '').trim(),
      workflowTone: workflowSnapshot.preview.badgeTone,
      workflowLines
    } satisfies SwarmProjectionNode;
  });

  if (!nodes.length) {
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

  const effectiveMotherAgentId = motherAgentId || orderedAgentIds[0] || '';
  const motherNodeId = effectiveMotherAgentId ? `agent:${effectiveMotherAgentId}` : '';
  const edges: SwarmProjectionEdge[] = [];
  if (effectiveMotherAgentId) {
    orderedAgentIds.forEach((agentId) => {
      if (!agentId || agentId === effectiveMotherAgentId) return;
      const agentTasks = tasksByAgent.get(agentId) || [];
      const latestTask = pickLatestTask(agentTasks);
      const latestStatus = String(latestTask?.status || '').trim().toLowerCase();
      const dispatchActive =
        ACTIVE_DISPATCH_STATUSES.has(latestStatus) ||
        (memberMap.get(agentId)?.idle === false && agentTasks.length > 0);
      const targetNodeId = `agent:${agentId}`;
      const edgeSelected = Boolean(options.selectedNodeId) &&
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
