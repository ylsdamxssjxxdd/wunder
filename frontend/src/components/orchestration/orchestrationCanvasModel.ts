import type { MissionChatMessage } from '@/components/beeroom/beeroomCanvasChatModel';
import type { BeeroomMissionSubagentItem } from '@/components/beeroom/beeroomMissionSubagentState';
import {
  buildNodeWorkflowPreviewLines,
  buildTaskWorkflowRuntime,
  type BeeroomTaskWorkflowPreview,
  type BeeroomWorkflowItem
} from '@/components/beeroom/beeroomTaskWorkflow';
import {
  ACTIVE_DISPATCH_STATUSES,
  NODE_HEIGHT,
  NODE_WIDTH,
  resolveBeeroomDispatchTaskLabel,
  type BeeroomSwarmDispatchPreview,
  type SwarmProjection,
  type SwarmProjectionBounds,
  type SwarmProjectionEdge,
  type SwarmProjectionNode
} from '@/components/beeroom/canvas/swarmCanvasModel';
import type { OrchestrationArtifactCard, OrchestrationRound } from '@/components/orchestration/orchestrationRuntimeState';
import type { BeeroomGroup, BeeroomMember, BeeroomMission, BeeroomMissionTask } from '@/stores/beeroom';
import {
  DEFAULT_AGENT_AVATAR_IMAGE,
  parseAgentAvatarIconConfig,
  resolveAgentAvatarConfiguredColor,
  resolveAgentAvatarImageByConfig,
  resolveAgentAvatarInitial
} from '@/utils/agentAvatar';

type TranslationFn = (key: string, params?: Record<string, unknown>) => string;

const ARTIFACT_NODE_WIDTH = 276;
const ARTIFACT_NODE_HEIGHT = 186;
const MOTHER_X = -420;
const WORKER_X = 0;
const ARTIFACT_X = 420;
const ROW_GAP = 228;
const ACTIVE_SUBAGENT_STATUSES = new Set(['running', 'waiting', 'queued', 'accepted', 'cancelling']);
const ACTIVE_MISSION_STATUSES = new Set(['queued', 'pending', 'running', 'awaiting_idle', 'resuming', 'merging']);
const ACTIVE_TASK_STATUSES = new Set([
  'queued',
  'pending',
  'running',
  'awaiting_idle',
  'resuming',
  'merging',
  'waiting',
  'accepted',
  'cancelling'
]);
const TERMINAL_TASK_STATUSES = new Set(['success', 'completed', 'failed', 'error', 'timeout', 'cancelled', 'canceled']);
const TERMINAL_MISSION_STATUSES = new Set(['success', 'completed', 'failed', 'error', 'timeout', 'cancelled', 'canceled']);

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

const resolveWorkflowToneRank = (tone: unknown) => {
  const normalized = normalizeText(tone).toLowerCase();
  if (normalized === 'loading') return 4;
  if (normalized === 'failed') return 3;
  if (normalized === 'completed') return 2;
  if (normalized === 'pending') return 1;
  return 0;
};

const resolveStatusFromWorkflowTone = (tone: unknown) => {
  const normalized = normalizeText(tone).toLowerCase();
  if (normalized === 'loading') return 'running';
  if (normalized === 'failed') return 'failed';
  if (normalized === 'completed') return 'completed';
  return '';
};

const resolveWorkerStatus = (
  workerId: string,
  activeRoundMissions: BeeroomMission[],
  workerOutputs: MissionChatMessage[],
  fallbackStatus: string,
  runtimeWorkerStatus: string,
  workflowTone: string
) => {
  const workflowStatus = resolveStatusFromWorkflowTone(workflowTone);
  const normalizedWorkerId = normalizeText(workerId);
  const missionTaskStatus = activeRoundMissions
    .flatMap((mission) => (Array.isArray(mission.tasks) ? mission.tasks : []))
    .filter((task) => normalizeText(task?.agent_id) === normalizedWorkerId)
    .map((task) => normalizeText(task?.status).toLowerCase())
    .find(Boolean);
  if (missionTaskStatus) {
    if (
      (missionTaskStatus === 'queued' || missionTaskStatus === 'pending' || missionTaskStatus === 'accepted' || missionTaskStatus === 'waiting') &&
      resolveWorkflowToneRank(workflowTone) > resolveWorkflowToneRank('pending')
    ) {
      return workflowStatus || missionTaskStatus;
    }
    return missionTaskStatus;
  }
  if (workflowStatus) return workflowStatus;
  if (runtimeWorkerStatus) return runtimeWorkerStatus;
  if (workerOutputs.length) return 'completed';
  return normalizeText(fallbackStatus).toLowerCase() || 'idle';
};

const isTerminalMissionStatus = (value: unknown) =>
  TERMINAL_MISSION_STATUSES.has(normalizeText(value).toLowerCase());

const isTerminalTaskStatus = (value: unknown) =>
  TERMINAL_TASK_STATUSES.has(normalizeText(value).toLowerCase());

const isActiveTaskStatus = (value: unknown) =>
  ACTIVE_TASK_STATUSES.has(normalizeText(value).toLowerCase());

const isMissionRunning = (mission: BeeroomMission) => {
  const completionStatus = normalizeText(mission?.completion_status).toLowerCase();
  const missionStatus = normalizeText(mission?.status).toLowerCase();
  if (
    isTerminalMissionStatus(completionStatus) ||
    isTerminalMissionStatus(missionStatus) ||
    Number(mission?.finished_time || 0) > 0
  ) {
    return false;
  }
  const tasks = Array.isArray(mission?.tasks) ? mission.tasks : [];
  if (
    tasks.length &&
    tasks.every(
      (task) =>
        isTerminalTaskStatus(task?.status) || Number(task?.finished_time || 0) > 0
    )
  ) {
    return false;
  }
  if (
    tasks.some((task) => {
      const taskStatus = normalizeText(task?.status).toLowerCase();
      if (isActiveTaskStatus(taskStatus)) return true;
      return !taskStatus && Number(task?.finished_time || 0) <= 0;
    })
  ) {
    return true;
  }
  return ACTIVE_MISSION_STATUSES.has(completionStatus || missionStatus);
};

const resolveTaskMoment = (task: BeeroomMissionTask | null | undefined) =>
  Math.max(
    Number(task?.updated_time || 0),
    Number(task?.finished_time || 0),
    Number(task?.started_time || 0)
  );

const pickLatestWorkerTask = (tasks: BeeroomMissionTask[]) =>
  tasks.reduce<BeeroomMissionTask | null>((latest, current) => {
    if (!latest) return current;
    return resolveTaskMoment(current) >= resolveTaskMoment(latest) ? current : latest;
  }, null);

const buildMotherWorkflowLines = (options: {
  motherNodeId: string;
  motherSessionId: string;
  activeRound: OrchestrationRound | null;
  motherWorkflowItems: BeeroomWorkflowItem[];
  runtimeDispatch: BeeroomSwarmDispatchPreview | null;
  t: TranslationFn;
}) => {
  const workflowLines = buildNodeWorkflowPreviewLines(options.motherWorkflowItems, {
    includeEventFallback: true
  });
  if (workflowLines.length > 0) {
    return workflowLines.slice(0, 3);
  }
  return [
    {
      key: `${options.motherNodeId}:thread`,
      main: options.t('orchestration.canvas.session', {
        id: normalizeText(options.motherSessionId).slice(0, 8) || '-'
      }),
      detail: trimText(
        options.activeRound?.situation || options.t('orchestration.canvas.noSituation'),
        28
      ),
      title: options.activeRound?.situation || options.t('orchestration.canvas.noSituation')
    },
    ...(isActiveStatus(options.runtimeDispatch?.status)
      ? [
          {
            key: `${options.motherNodeId}:dispatching`,
            main: options.t('orchestration.canvas.motherDispatching'),
            detail: options.t('beeroom.status.running'),
            title: options.t('orchestration.canvas.motherDispatching')
          }
        ]
      : [])
  ];
};

const buildWorkerWorkflowSnapshot = (options: {
  task: BeeroomMissionTask | null;
  workflowItemsByTask: Record<string, BeeroomWorkflowItem[]>;
  workflowPreviewByTask: Record<string, BeeroomTaskWorkflowPreview>;
  t: TranslationFn;
}) => {
  const fallbackRuntime = buildTaskWorkflowRuntime(options.task, [], options.t);
  if (!options.task) {
    return {
      items: fallbackRuntime.items,
      tone: fallbackRuntime.preview.badgeTone
    };
  }
  const taskId = normalizeText(options.task.task_id);
  const items = taskId ? options.workflowItemsByTask[taskId] : null;
  if (Array.isArray(items) && items.length > 0) {
    return {
      items,
      tone: options.workflowPreviewByTask[taskId]?.badgeTone || fallbackRuntime.preview.badgeTone
    };
  }
  return {
    items: fallbackRuntime.items,
    tone: fallbackRuntime.preview.badgeTone
  };
};

const resolveWorkerEdgeLabel = (options: {
  workerId: string;
  mission: BeeroomMission | null;
  task: BeeroomMissionTask | null;
  runtimeDispatch: BeeroomSwarmDispatchPreview | null;
}) => {
  const runtimeTargetAgentId = normalizeText(options.runtimeDispatch?.targetAgentId);
  if (runtimeTargetAgentId === options.workerId && isActiveStatus(options.runtimeDispatch?.status)) {
    return trimText(options.runtimeDispatch?.dispatchLabel || options.runtimeDispatch?.summary, 18);
  }
  const task = options.task;
  if (!task) return '';
  const taskSummary = trimText(task.result_summary || task.error || '', 18);
  if (taskSummary) return taskSummary;
  if (isActiveTaskStatus(task.status) && options.mission) {
    return trimText(
      resolveBeeroomDispatchTaskLabel(options.mission, task, {
        allowTerminalTaskFallback: false
      }),
      18
    );
  }
  return '';
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
  motherWorkflowItems: BeeroomWorkflowItem[];
  workflowItemsByTask: Record<string, BeeroomWorkflowItem[]>;
  workflowPreviewByTask: Record<string, BeeroomTaskWorkflowPreview>;
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
  const tasksByAgent = new Map<string, BeeroomMissionTask[]>();
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
  options.activeRoundMissions.forEach((mission) => {
    const missionTasks = Array.isArray(mission?.tasks) ? mission.tasks : [];
    missionTasks.forEach((task) => {
      const agentId = normalizeText(task?.agent_id);
      if (!agentId) return;
      const bucket = tasksByAgent.get(agentId) || [];
      bucket.push(task);
      tasksByAgent.set(agentId, bucket);
    });
  });
  const hasRunningMission = options.activeRoundMissions.some((mission) => isMissionRunning(mission));
  const motherStatus = hasRunningMission || isActiveStatus(runtimeDispatch?.status) ? 'running' : 'idle';
  const motherStatusLabel = resolveNodeStatusLabel(motherStatus, options.t);
  const motherWorkflowLines = buildMotherWorkflowLines({
    motherNodeId,
    motherSessionId: options.motherSessionId,
    activeRound: options.activeRound,
    motherWorkflowItems: options.motherWorkflowItems,
    runtimeDispatch,
    t: options.t
  });
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
      workflowTone: isActiveStatus(runtimeDispatch?.status) || hasRunningMission ? 'loading' : 'pending',
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
    const workerTasks = tasksByAgent.get(agentId) || [];
    const latestWorkerTask = pickLatestWorkerTask(workerTasks);
    const workflowSnapshot = buildWorkerWorkflowSnapshot({
      task: latestWorkerTask,
      workflowItemsByTask: options.workflowItemsByTask,
      workflowPreviewByTask: options.workflowPreviewByTask,
      t: options.t
    });
    const workerStatus = resolveWorkerStatus(
      agentId,
      options.activeRoundMissions,
      workerOutputs,
      member?.idle === false ? 'running' : 'idle',
      runtimeWorkerStatus,
      workflowSnapshot.tone
    );
    const workerActive =
      workerTasks.some((task) => isActiveTaskStatus(task?.status)) ||
      workerStatus === 'running' ||
      workerStatus === 'queued' ||
      workerStatus === 'awaiting_idle';
    const workerStatusLabel = resolveNodeStatusLabel(workerStatus, options.t);
    const workerName = normalizeText(member?.name) || agentId;
    const workerOverride = options.nodePositionOverrides[workerNodeId];
    const workerY = index * ROW_GAP;
    const threadShort = normalizeText(options.resolveWorkerThreadSessionId(agentId)).slice(0, 8) || '-';
    const workflowLines = buildNodeWorkflowPreviewLines(workflowSnapshot.items, {
      includeEventFallback: true
    }).slice(0, 3);
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
        workflowLines.length > 0
          ? workflowSnapshot.tone
          : workerStatus === 'completed' || workerStatus === 'success'
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
      task_total: workerTasks.length,
      active_session_total: 1,
      updated_time: Number(options.activeRound?.createdAt || 0),
      summary: workerOutputs[0]?.body || workflowLines[0]?.title || '',
      entry_agent: false,
      parent_id: motherNodeId,
      emphasis: workerActive ? 'active' : 'default'
    });
    const edgeMission =
      options.activeRoundMissions.find((mission) =>
        (Array.isArray(mission?.tasks) ? mission.tasks : []).some(
          (task) => normalizeText(task?.agent_id) === agentId
        )
      ) || null;
    edges.push({
      id: `dispatch:${motherNodeId}:${workerNodeId}`,
      source: motherNodeId,
      target: workerNodeId,
      label: resolveWorkerEdgeLabel({
        workerId: agentId,
        mission: edgeMission,
        task: latestWorkerTask,
        runtimeDispatch
      }),
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
      artifactItems: (artifactCard?.entries || []).map((entry, entryIndex) => ({
        key: `${artifactNodeId}:artifact:${entryIndex}`,
        label: trimText(entry.name, 12) || '-',
        title: entry.path || entry.name || '',
        meta: resolveArtifactMeta(entry),
        kind: entry.type === 'dir' ? 'dir' : 'file',
        iconClass: resolveArtifactIconClass(entry),
        path: entry.path,
        name: entry.name,
        size: entry.size,
        updatedTime: entry.updatedTime,
        updatedAtMs: entry.updatedAtMs,
        preview: entry.preview,
        previewable: entry.type !== 'dir'
      })),
      artifactPath: artifactCard?.path || '',
      artifactCount: artifactCard?.entries?.length || 0,
      artifactDisplayMode: 'showcase',
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
