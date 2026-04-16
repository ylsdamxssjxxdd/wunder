import type { MissionChatMessage } from '@/components/beeroom/beeroomCanvasChatModel';
import type { BeeroomMissionSubagentItem } from '@/components/beeroom/beeroomMissionSubagentState';
import {
  ACTIVE_DISPATCH_STATUSES,
  NODE_HEIGHT,
  NODE_WIDTH,
  type BeeroomSwarmDispatchPreview,
  type SwarmProjection,
  type SwarmProjectionBounds,
  type SwarmProjectionEdge,
  type SwarmProjectionNode
} from '@/components/beeroom/canvas/swarmCanvasModel';
import type { OrchestrationArtifactCard, OrchestrationRound } from '@/components/orchestration/orchestrationRuntimeState';
import type { BeeroomGroup, BeeroomMember, BeeroomMission } from '@/stores/beeroom';
import {
  DEFAULT_AGENT_AVATAR_IMAGE,
  parseAgentAvatarIconConfig,
  resolveAgentAvatarConfiguredColor,
  resolveAgentAvatarImageByConfig,
  resolveAgentAvatarInitial
} from '@/utils/agentAvatar';

type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

const ARTIFACT_NODE_WIDTH = 252;
const ARTIFACT_NODE_HEIGHT = 154;
const MOTHER_X = -420;
const WORKER_X = 0;
const ARTIFACT_X = 420;
const ROW_GAP = 228;
const ACTIVE_SUBAGENT_STATUSES = new Set(['running', 'waiting', 'queued', 'accepted', 'cancelling']);

const normalizeText = (value: unknown) => String(value || '').trim();

const trimText = (value: unknown, max: number) => {
  const text = normalizeText(value);
  if (!text) return '';
  if (text.length <= max) return text;
  return `${text.slice(0, max)}...`;
};

const resolveArtifactIconClass = (entry: { type?: unknown; name?: unknown; path?: unknown }) => {
  const entryType = normalizeText(entry?.type).toLowerCase();
  if (entryType === 'dir') return 'fa-folder';
  const name = normalizeText(entry?.name || entry?.path).toLowerCase();
  const extension = name.split('.').pop() || '';
  if (['md', 'txt', 'log', 'csv'].includes(extension)) return 'fa-file-lines';
  if (['json', 'yaml', 'yml', 'toml', 'xml'].includes(extension)) return 'fa-file-code';
  if (['ts', 'tsx', 'js', 'jsx', 'rs', 'py', 'java', 'go', 'vue', 'html', 'css', 'scss'].includes(extension)) {
    return 'fa-file-code';
  }
  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg'].includes(extension)) return 'fa-file-image';
  if (['pdf', 'doc', 'docx', 'ppt', 'pptx', 'xls', 'xlsx'].includes(extension)) return 'fa-file';
  if (['zip', '7z', 'rar', 'tar', 'gz'].includes(extension)) return 'fa-file-zipper';
  return 'fa-file';
};

const resolveArtifactMeta = (entry: {
  type?: unknown;
  preview?: unknown;
  updatedTime?: unknown;
  path?: unknown;
}) => {
  const entryType = normalizeText(entry?.type).toLowerCase();
  if (entryType === 'dir') return 'dir';
  return trimText(entry?.updatedTime || entry?.preview || entry?.path, 16);
};

const resolveAvatarImage = (icon: unknown) => {
  const text = normalizeText(icon);
  if (!text) return '';
  return resolveAgentAvatarImageByConfig(parseAgentAvatarIconConfig(icon));
};

const resolveNodeStatusLabel = (status: string, t: TranslationFn) => {
  const normalized = normalizeText(status).toLowerCase();
  if (normalized === 'queued') return t('beeroom.status.queued');
  if (normalized === 'running') return t('beeroom.status.running');
  if (normalized === 'awaiting_idle') return t('beeroom.status.awaitingIdle');
  if (normalized === 'completed' || normalized === 'success') return t('beeroom.status.completed');
  if (normalized === 'failed' || normalized === 'error' || normalized === 'timeout') return t('beeroom.status.failed');
  if (normalized === 'cancelled' || normalized === 'canceled') return t('beeroom.status.cancelled');
  if (normalized === 'idle') return t('beeroom.members.idle');
  return t('beeroom.status.unknown');
};

const resolveWorkerStatus = (
  workerId: string,
  activeRoundMissions: BeeroomMission[],
  workerOutputs: MissionChatMessage[],
  fallbackStatus: string,
  runtimeWorkerStatus: string
) => {
  const normalizedWorkerId = normalizeText(workerId);
  const missionTaskStatus = activeRoundMissions
    .flatMap((mission) => (Array.isArray(mission.tasks) ? mission.tasks : []))
    .filter((task) => normalizeText(task?.agent_id) === normalizedWorkerId)
    .map((task) => normalizeText(task?.status).toLowerCase())
    .find(Boolean);
  if (missionTaskStatus) return missionTaskStatus;
  if (runtimeWorkerStatus) return runtimeWorkerStatus;
  if (workerOutputs.length) return 'completed';
  return normalizeText(fallbackStatus).toLowerCase() || 'idle';
};

const resolveDispatchPreviewStatus = (value: unknown) => {
  const normalized = normalizeText(value).toLowerCase();
  if (normalized === 'resuming' || normalized === 'awaiting_approval') return 'running';
  if (normalized === 'waiting' || normalized === 'accepted') return 'queued';
  if (normalized === 'canceling' || normalized === 'cancelling' || normalized === 'stopped') return 'cancelled';
  if (normalized === 'success') return 'completed';
  if (normalized === 'error' || normalized === 'timeout') return 'failed';
  return normalized;
};

const isActiveStatus = (value: unknown) => {
  const normalized = resolveDispatchPreviewStatus(value);
  return ACTIVE_DISPATCH_STATUSES.has(normalized) || ACTIVE_SUBAGENT_STATUSES.has(normalized);
};

const resolveSubagentAgentId = (item: BeeroomMissionSubagentItem) => normalizeText(item.agentId);

const resolveRuntimeWorkers = (
  dispatchPreview: BeeroomSwarmDispatchPreview | null | undefined,
  motherAgentId: string
) => {
  const workerIds = new Set<string>();
  const statusByAgentId = new Map<string, string>();
  const previewStatus = resolveDispatchPreviewStatus(dispatchPreview?.status);
  const previewTargetAgentId = normalizeText(dispatchPreview?.targetAgentId);
  if (previewTargetAgentId && previewTargetAgentId !== motherAgentId) {
    workerIds.add(previewTargetAgentId);
    if (previewStatus) {
      statusByAgentId.set(previewTargetAgentId, previewStatus);
    }
  }
  (Array.isArray(dispatchPreview?.subagents) ? dispatchPreview.subagents : []).forEach((item) => {
    const agentId = resolveSubagentAgentId(item);
    if (!agentId || agentId === motherAgentId) return;
    workerIds.add(agentId);
    const status = resolveDispatchPreviewStatus(item.status);
    if (status && (!statusByAgentId.has(agentId) || isActiveStatus(status))) {
      statusByAgentId.set(agentId, status);
    }
  });
  return { workerIds, statusByAgentId };
};

const computeBounds = (nodes: SwarmProjectionNode[]): SwarmProjectionBounds => {
  if (!nodes.length) {
    return {
      minX: 0,
      minY: 0,
      maxX: 0,
      maxY: 0,
      width: 0,
      height: 0
    };
  }
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  nodes.forEach((node) => {
    minX = Math.min(minX, node.x - node.width / 2);
    minY = Math.min(minY, node.y - node.height / 2);
    maxX = Math.max(maxX, node.x + node.width / 2);
    maxY = Math.max(maxY, node.y + node.height / 2);
  });
  return {
    minX,
    minY,
    maxX,
    maxY,
    width: Math.max(0, maxX - minX),
    height: Math.max(0, maxY - minY)
  };
};

export const buildOrchestrationCanvasScopeKey = (runId: string, roundId: string) =>
  `orchestration:${normalizeText(runId) || 'standby'}:${normalizeText(roundId) || 'active'}`;

export const buildOrchestrationCanvasProjection = (options: {
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  motherAgentId: string;
  motherName: string;
  motherSessionId: string;
  activeRound: OrchestrationRound | null;
  activeRoundMissions: BeeroomMission[];
  visibleWorkers: BeeroomMember[];
  artifactCards: OrchestrationArtifactCard[];
  dispatchPreview?: BeeroomSwarmDispatchPreview | null;
  resolveWorkerOutputs: (agentId: string) => MissionChatMessage[];
  resolveWorkerThreadSessionId: (agentId: string) => string;
  selectedNodeId: string;
  nodePositionOverrides: Record<string, { x: number; y: number }>;
  resolveAgentAvatarImageByAgentId?: (agentId: unknown) => string;
  resolveAgentAvatarColorByAgentId?: (agentId: unknown) => string;
  t: TranslationFn;
}): SwarmProjection => {
  const nodeMetaMap = new Map();
  const memberMap = new Map<string, BeeroomMember>();
  const tasksByAgent = new Map();
  (Array.isArray(options.agents) ? options.agents : []).forEach((member) => {
    const agentId = normalizeText(member?.agent_id);
    if (!agentId) return;
    memberMap.set(agentId, member);
  });

  const nodes: SwarmProjectionNode[] = [];
  const edges: SwarmProjectionEdge[] = [];
  const runtimeDispatch = options.dispatchPreview || null;
  const motherId = normalizeText(options.motherAgentId);
  const motherNodeId = motherId ? `agent:${motherId}` : 'mother:standby';
  const motherMember = motherId ? memberMap.get(motherId) || null : null;
  const motherOverride = options.nodePositionOverrides[motherNodeId];
  const workerCenterY = options.visibleWorkers.length > 1 ? ((options.visibleWorkers.length - 1) * ROW_GAP) / 2 : 0;
  const runtimeWorkers = resolveRuntimeWorkers(runtimeDispatch, motherId);
  const motherStatus = options.activeRoundMissions.length || isActiveStatus(runtimeDispatch?.status) ? 'running' : 'idle';
  const motherStatusLabel = resolveNodeStatusLabel(motherStatus, options.t);
  const motherWorkflowLines = [
    {
      key: `${motherNodeId}:thread`,
      main: options.t('orchestration.canvas.session', {
        id: normalizeText(options.motherSessionId).slice(0, 8) || '-'
      }),
      detail: trimText(options.activeRound?.situation || options.t('orchestration.canvas.noSituation'), 28),
      title: options.activeRound?.situation || options.t('orchestration.canvas.noSituation')
    },
    ...(isActiveStatus(runtimeDispatch?.status)
      ? [
          {
            key: `${motherNodeId}:dispatching`,
            main: options.t('orchestration.canvas.motherDispatching'),
            detail: options.t('beeroom.status.running'),
            title: options.t('orchestration.canvas.motherDispatching')
          }
        ]
      : [])
  ];
  nodes.push({
    id: motherNodeId,
    agentId: motherId,
    name: options.motherName,
    displayName: trimText(options.motherName, 12) || '-',
    role: 'mother',
    roleLabel: options.t('beeroom.canvas.legendMother'),
    status: motherStatus,
    statusLabel: motherStatusLabel,
    selected: motherNodeId === normalizeText(options.selectedNodeId),
    accentColor: '#f59e0b',
    avatarColor:
      normalizeText(options.resolveAgentAvatarColorByAgentId?.(motherId)) ||
      resolveAgentAvatarConfiguredColor(motherMember?.icon) ||
      '#f59e0b',
    avatarInitial: resolveAgentAvatarInitial(options.motherName),
    avatarImageUrl:
      resolveAvatarImage(motherMember?.icon) ||
      normalizeText(options.resolveAgentAvatarImageByAgentId?.(motherId)) ||
      DEFAULT_AGENT_AVATAR_IMAGE,
    x: Number(motherOverride?.x ?? MOTHER_X),
      y: Number(motherOverride?.y ?? workerCenterY),
      width: NODE_WIDTH,
      height: NODE_HEIGHT,
      workflowTaskId: '',
      workflowTone: isActiveStatus(runtimeDispatch?.status) || options.activeRoundMissions.length ? 'loading' : 'pending',
      workflowLines: motherWorkflowLines,
    parentId: '',
    emphasis: 'default',
    introFromId: '',
    introOrder: 0
  });
  nodeMetaMap.set(motherNodeId, {
    id: motherNodeId,
    agent_id: motherId,
    agent_name: options.motherName,
    role: 'mother',
    role_label: options.t('beeroom.canvas.legendMother'),
    status: motherStatus,
    task_total: options.activeRoundMissions.length,
    active_session_total: 1,
    updated_time: Number(options.activeRound?.createdAt || 0),
    summary: options.activeRound?.situation || '',
    entry_agent: false,
    parent_id: '',
    emphasis: 'default'
  });

  options.visibleWorkers.forEach((member, index) => {
    const agentId = normalizeText(member?.agent_id);
    if (!agentId) return;
    const workerNodeId = `agent:${agentId}`;
    const workerOutputs = options.resolveWorkerOutputs(agentId);
    const runtimeWorkerStatus = runtimeWorkers.statusByAgentId.get(agentId) || '';
    const workerStatus = resolveWorkerStatus(
      agentId,
      options.activeRoundMissions,
      workerOutputs,
      member?.idle === false ? 'running' : 'idle',
      runtimeWorkerStatus
    );
    const workerActive =
      workerStatus === 'running' || workerStatus === 'queued' || workerStatus === 'awaiting_idle';
    const workerStatusLabel = resolveNodeStatusLabel(workerStatus, options.t);
    const workerName = normalizeText(member?.name) || agentId;
    const workerOverride = options.nodePositionOverrides[workerNodeId];
    const workerY = index * ROW_GAP;
    const threadShort = normalizeText(options.resolveWorkerThreadSessionId(agentId)).slice(0, 8) || '-';
    const workflowLines =
      workerOutputs.slice(0, 3).map((item, outputIndex) => ({
        key: `${workerNodeId}:output:${outputIndex}`,
        main: trimText(item.body, 14) || options.t('orchestration.canvas.noWorkerOutput'),
        detail: item.timeLabel || workerStatusLabel,
        title: item.body
      })) || [];
    nodes.push({
      id: workerNodeId,
      agentId,
      name: workerName,
      displayName: trimText(workerName, 12) || '-',
      role: 'worker',
      roleLabel: options.t('beeroom.canvas.legendWorker'),
      status: workerStatus,
      statusLabel: workerStatusLabel,
      selected: workerNodeId === normalizeText(options.selectedNodeId),
      accentColor:
        normalizeText(options.resolveAgentAvatarColorByAgentId?.(agentId)) ||
        resolveAgentAvatarConfiguredColor(member?.icon) ||
        '#3b82f6',
      avatarColor:
        normalizeText(options.resolveAgentAvatarColorByAgentId?.(agentId)) ||
        resolveAgentAvatarConfiguredColor(member?.icon) ||
        '#3b82f6',
      avatarInitial: resolveAgentAvatarInitial(workerName),
      avatarImageUrl:
        resolveAvatarImage(member?.icon) ||
        normalizeText(options.resolveAgentAvatarImageByAgentId?.(agentId)),
      x: Number(workerOverride?.x ?? WORKER_X),
      y: Number(workerOverride?.y ?? workerY),
      width: NODE_WIDTH,
      height: NODE_HEIGHT,
      workflowTaskId: '',
      workflowTone:
        workerStatus === 'completed' || workerStatus === 'success'
          ? 'completed'
          : workerStatus === 'failed'
            ? 'failed'
            : workerActive
              ? 'loading'
              : 'pending',
      workflowLines: workflowLines.length
        ? workflowLines
        : [
            ...(workerActive
              ? [
                  {
                    key: `${workerNodeId}:dispatching`,
                    main: options.t('orchestration.canvas.workerDispatchPending'),
                    detail: options.t('orchestration.canvas.workerDispatchRunning'),
                    title: options.t('orchestration.canvas.workerDispatchRunning')
                  }
                ]
              : [
                  {
                    key: `${workerNodeId}:thread`,
                    main: options.t('orchestration.canvas.session', { id: threadShort }),
                    detail: options.t('orchestration.canvas.noWorkerOutput'),
                    title: options.t('orchestration.canvas.noWorkerOutput')
                  }
                ])
          ],
      parentId: '',
      emphasis: workerActive ? 'active' : 'default',
      introFromId: motherNodeId,
      introOrder: index + 1
    });
    nodeMetaMap.set(workerNodeId, {
      id: workerNodeId,
      agent_id: agentId,
      agent_name: workerName,
      role: 'worker',
      role_label: options.t('beeroom.canvas.legendWorker'),
      status: workerStatus,
      task_total: options.activeRoundMissions
        .flatMap((mission) => (Array.isArray(mission.tasks) ? mission.tasks : []))
        .filter((task) => normalizeText(task?.agent_id) === agentId).length,
      active_session_total: 1,
      updated_time: Number(options.activeRound?.createdAt || 0),
      summary: workerOutputs[0]?.body || '',
      entry_agent: false,
      parent_id: motherNodeId,
      emphasis: workerActive ? 'active' : 'default'
    });
    edges.push({
      id: `dispatch:${motherNodeId}:${workerNodeId}`,
      source: motherNodeId,
      target: workerNodeId,
      label: options.activeRoundMissions.length ? trimText(options.activeRoundMissions[0]?.summary || options.activeRoundMissions[0]?.strategy, 18) : '',
      active: workerActive,
      selected:
        normalizeText(options.selectedNodeId) === motherNodeId ||
        normalizeText(options.selectedNodeId) === workerNodeId,
      kind: 'dispatch'
    });

    const artifactCard = options.artifactCards.find((item) => normalizeText(item.agentId) === agentId) || null;
    const artifactNodeId = `artifact:${agentId}`;
    const artifactOverride = options.nodePositionOverrides[artifactNodeId];
    const artifactPreview = artifactCard?.entries?.[0] || null;
    const artifactStatus = artifactCard?.error
      ? 'failed'
      : artifactCard?.entries?.length
        ? 'completed'
        : workerActive
          ? 'queued'
          : 'idle';
    nodes.push({
      id: artifactNodeId,
      agentId,
      name: artifactCard?.agentName || workerName,
      displayName: trimText(artifactCard?.agentName || workerName, 12) || '-',
      renderKind: 'artifact-container',
      role: 'subagent',
      roleLabel: options.t('orchestration.canvas.artifact'),
      status: artifactStatus,
      statusLabel: resolveNodeStatusLabel(artifactStatus, options.t),
      selected: artifactNodeId === normalizeText(options.selectedNodeId),
      accentColor: '#10b981',
      avatarColor: '#10b981',
      avatarInitial: resolveAgentAvatarInitial(artifactCard?.agentName || workerName),
      avatarImageUrl: '',
      x: Number(artifactOverride?.x ?? ARTIFACT_X),
      y: Number(artifactOverride?.y ?? workerY),
      width: ARTIFACT_NODE_WIDTH,
      height: ARTIFACT_NODE_HEIGHT,
      workflowTaskId: '',
      workflowTone: artifactCard?.error
        ? 'failed'
        : artifactCard?.entries?.length
          ? 'completed'
          : workerActive
            ? 'loading'
            : 'pending',
      artifactItems: (artifactCard?.entries || []).slice(0, 8).map((entry, entryIndex) => ({
        key: `${artifactNodeId}:artifact:${entryIndex}`,
        label: trimText(entry.name, 12) || '-',
        title: entry.path || entry.name || '',
        meta: resolveArtifactMeta(entry),
        kind: entry.type === 'dir' ? 'dir' : 'file',
        iconClass: resolveArtifactIconClass(entry)
      })),
      artifactPath: artifactCard?.path || '',
      artifactCount: artifactCard?.entries?.length || 0,
      workflowLines: artifactCard?.entries?.length
        ? artifactCard.entries.slice(0, 3).map((entry, entryIndex) => ({
            key: `${artifactNodeId}:entry:${entryIndex}`,
            main: trimText(entry.name, 14) || '-',
            detail: trimText(entry.preview || entry.updatedTime || entry.path, 24),
            title: entry.path
          }))
        : [
            {
              key: `${artifactNodeId}:empty`,
              main: workerActive
                ? options.t('orchestration.canvas.artifactPending')
                : options.t('orchestration.canvas.noArtifacts'),
              detail: trimText(artifactCard?.path || '', 24),
              title: artifactCard?.path || ''
            }
          ],
      parentId: workerNodeId,
      emphasis: artifactCard?.entries?.length || workerActive ? 'active' : 'dormant',
      introFromId: workerNodeId,
      introOrder: index + 1
    });
    nodeMetaMap.set(artifactNodeId, {
      id: artifactNodeId,
      agent_id: agentId,
      agent_name: artifactCard?.agentName || workerName,
      role: 'subagent',
      role_label: options.t('orchestration.canvas.artifact'),
      status: artifactStatus,
      task_total: artifactCard?.entries?.length || 0,
      active_session_total: 0,
      updated_time: Number(options.activeRound?.createdAt || 0),
      summary: artifactPreview?.preview || artifactPreview?.path || artifactCard?.path || '',
      entry_agent: false,
      parent_id: workerNodeId,
      emphasis: artifactCard?.entries?.length || workerActive ? 'active' : 'dormant'
    });
    edges.push({
      id: `artifact:${workerNodeId}:${artifactNodeId}`,
      source: workerNodeId,
      target: artifactNodeId,
      label: artifactCard?.entries?.length ? trimText(artifactPreview?.name || '', 18) : '',
      active: Boolean(artifactCard?.entries?.length) || workerActive,
      selected:
        normalizeText(options.selectedNodeId) === workerNodeId ||
        normalizeText(options.selectedNodeId) === artifactNodeId,
      kind: 'subagent'
    });
  });

  return {
    nodes,
    edges,
    nodeMetaMap,
    memberMap,
    tasksByAgent,
    motherNodeId,
    bounds: computeBounds(nodes)
  };
};
