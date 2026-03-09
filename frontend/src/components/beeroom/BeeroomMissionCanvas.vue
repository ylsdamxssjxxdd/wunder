<template>
  <section class="beeroom-panel beeroom-panel--canvas">
    <div class="beeroom-panel-head">
      <div>
        <div class="beeroom-panel-title">{{ t('beeroom.canvas.title') }}</div>
        <div class="beeroom-panel-subtitle">{{ t('beeroom.canvas.subtitle') }}</div>
      </div>
      <div v-if="mission" class="beeroom-canvas-actions">
        <button class="beeroom-action-btn" type="button" @click="focusMother">
          <i class="fa-solid fa-crosshairs" aria-hidden="true"></i>
          <span>{{ t('beeroom.canvas.focusMother') }}</span>
        </button>
        <button class="beeroom-action-btn" type="button" @click="fitCanvas">
          <i class="fa-solid fa-up-right-and-down-left-from-center" aria-hidden="true"></i>
          <span>{{ t('beeroom.canvas.fitView') }}</span>
        </button>
      </div>
    </div>

    <div v-if="!mission" class="beeroom-panel-empty">{{ t('beeroom.canvas.empty') }}</div>
    <div v-else class="beeroom-canvas-layout">
      <div class="beeroom-canvas-surface-wrap">
        <div class="beeroom-canvas-legend">
          <span class="beeroom-canvas-legend-item"><i class="dot mother"></i>{{ t('beeroom.canvas.legendMother') }}</span>
          <span class="beeroom-canvas-legend-item"><i class="dot worker"></i>{{ t('beeroom.canvas.legendWorker') }}</span>
          <span class="beeroom-canvas-legend-item"><i class="line dispatch"></i>{{ t('beeroom.canvas.legendDispatch') }}</span>
          <span class="beeroom-canvas-legend-item"><i class="line report"></i>{{ t('beeroom.canvas.legendReport') }}</span>
          <span class="beeroom-canvas-legend-item"><i class="chip awaiting"></i>{{ t('beeroom.canvas.legendAwaitingIdle') }}</span>
        </div>
        <div ref="canvasRef" class="beeroom-canvas-surface"></div>
      </div>

      <aside class="beeroom-canvas-sidebar">
        <section class="beeroom-canvas-card">
          <div class="beeroom-canvas-card-title">{{ t('beeroom.canvas.overview') }}</div>
          <template v-if="missionOverview.length">
            <div class="beeroom-canvas-overview-grid">
              <div
                v-for="item in missionOverview"
                :key="item.key"
                class="beeroom-canvas-overview-item"
              >
                <div class="beeroom-canvas-overview-label">{{ item.label }}</div>
                <div class="beeroom-canvas-overview-value">{{ item.value }}</div>
              </div>
            </div>
          </template>
          <div v-else class="beeroom-canvas-node-empty">{{ t('beeroom.canvas.empty') }}</div>
        </section>

        <section class="beeroom-canvas-card">
          <div class="beeroom-canvas-card-title">{{ t('beeroom.canvas.nodeDetail') }}</div>
          <template v-if="activeNodeMeta">
            <div class="beeroom-canvas-node-name-row">
              <div>
                <div class="beeroom-canvas-node-name">{{ activeNodeMeta.agent_name }}</div>
                <div class="beeroom-canvas-node-role">
                  {{ activeNodeMeta.role_label }} · {{ resolveStatusLabel(activeNodeMeta.status) }}
                </div>
              </div>
              <button
                v-if="activeNodeMeta.agent_id"
                class="beeroom-inline-link"
                type="button"
                @click="$emit('open-agent', activeNodeMeta.agent_id)"
              >
                {{ t('beeroom.canvas.openChat') }}
              </button>
            </div>
            <div class="beeroom-canvas-node-grid">
              <div class="beeroom-canvas-node-field">
                <span>{{ t('beeroom.canvas.currentTaskTotal', { count: activeNodeMeta.task_total }) }}</span>
              </div>
              <div class="beeroom-canvas-node-field">
                <span>{{ t('beeroom.canvas.currentStatus') }}：{{ resolveStatusLabel(activeNodeMeta.status) }}</span>
              </div>
              <div class="beeroom-canvas-node-field">
                <span>{{ t('beeroom.canvas.activeSessions') }}：{{ activeNodeMeta.active_session_total }}</span>
              </div>
              <div class="beeroom-canvas-node-field">
                <span>{{ t('beeroom.canvas.lastUpdate') }}：{{ formatDateTime(activeNodeMeta.updated_time) }}</span>
              </div>
            </div>
            <div v-if="activeNodeMeta.summary" class="beeroom-canvas-node-summary">
              {{ activeNodeMeta.summary }}
            </div>
            <div v-if="activeNodeMeta.entry_agent" class="beeroom-canvas-node-hint">
              {{ t('beeroom.canvas.entryAgent') }}
            </div>
          </template>
          <div v-else class="beeroom-canvas-node-empty">{{ t('beeroom.canvas.noNode') }}</div>
        </section>

        <section class="beeroom-canvas-card">
          <div class="beeroom-canvas-card-title">{{ t('beeroom.canvas.timeline') }}</div>
          <div v-if="!timeline.length" class="beeroom-canvas-node-empty">
            {{ t('beeroom.canvas.timelineEmpty') }}
          </div>
          <div v-else class="beeroom-canvas-timeline">
            <article v-for="event in timeline" :key="event.id" class="beeroom-canvas-timeline-item">
              <div class="beeroom-canvas-timeline-time">{{ formatDateTime(event.time) }}</div>
              <div class="beeroom-canvas-timeline-body">
                <div class="beeroom-canvas-timeline-title">{{ event.title }}</div>
                <div class="beeroom-canvas-timeline-desc">{{ event.description }}</div>
              </div>
            </article>
          </div>
        </section>
      </aside>
    </div>
  </section>
</template>

<script setup lang="ts">
import { Graph } from '@antv/g6';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from '@/i18n';

import type { BeeroomGroup, BeeroomMember, BeeroomMission, BeeroomMissionTask } from '@/stores/beeroom';

type CanvasNodeMeta = {
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

type TimelineEvent = {
  id: string;
  time: number;
  title: string;
  description: string;
};

const props = defineProps<{
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
}>();

const emit = defineEmits<{
  (event: 'open-agent', agentId: string): void;
}>();

const { t } = useI18n();
const canvasRef = ref<HTMLDivElement | null>(null);
const activeNodeId = ref('');

let graph: Graph | null = null;
let resizeObserver: ResizeObserver | null = null;

const formatDateTime = (value: unknown) => {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric) || numeric <= 0) return '-';
  return new Intl.DateTimeFormat(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit'
  }).format(new Date(numeric * 1000));
};

const resolveStatusLabel = (value: unknown) => {
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

const projection = computed(() => {
  const mission = props.mission;
  if (!mission) {
    return {
      nodes: [] as any[],
      edges: [] as any[],
      nodeMetaMap: new Map<string, CanvasNodeMeta>(),
      motherNodeId: '',
      timeline: [] as TimelineEvent[]
    };
  }

  const tasks = Array.isArray(mission.tasks) ? mission.tasks : [];
  const agentMap = new Map(
    (Array.isArray(props.agents) ? props.agents : []).map((agent) => [String(agent.agent_id || '').trim(), agent])
  );
  const motherAgentId =
    String(mission.mother_agent_id || props.group?.mother_agent_id || mission.entry_agent_id || tasks[0]?.agent_id || '').trim();
  const involvedAgentIds = new Set<string>();
  tasks.forEach((task) => {
    const agentId = String(task.agent_id || '').trim();
    if (agentId) involvedAgentIds.add(agentId);
  });
  if (motherAgentId) involvedAgentIds.add(motherAgentId);
  const entryAgentId = String(mission.entry_agent_id || '').trim();
  if (entryAgentId) involvedAgentIds.add(entryAgentId);

  const tasksByAgent = new Map<string, BeeroomMissionTask[]>();
  tasks.forEach((task) => {
    const agentId = String(task.agent_id || '').trim();
    if (!agentId) return;
    const bucket = tasksByAgent.get(agentId) || [];
    bucket.push(task);
    tasksByAgent.set(agentId, bucket);
  });

  const missionStatus = String(mission.completion_status || mission.status || '').trim().toLowerCase();
  const nodeMetaMap = new Map<string, CanvasNodeMeta>();
  const nodes = Array.from(involvedAgentIds)
    .filter(Boolean)
    .map((agentId) => {
      const member = agentMap.get(agentId);
      const agentTasks = tasksByAgent.get(agentId) || [];
      const isMother = agentId === motherAgentId;
      const status = resolveNodeStatus(agentTasks, member, missionStatus);
      const tone =
        status === 'completed'
          ? { fill: '#dcfce7', stroke: '#16a34a' }
          : status === 'failed' || status === 'cancelled'
            ? { fill: '#fee2e2', stroke: '#dc2626' }
            : status === 'awaiting_idle'
              ? { fill: '#fef3c7', stroke: '#d97706' }
              : status === 'running'
                ? { fill: '#dbeafe', stroke: '#2563eb' }
                : { fill: '#e2e8f0', stroke: '#64748b' };
      const nodeId = `agent:${agentId}`;
      const name = String(member?.name || agentId);
      const summary =
        String(
          agentTasks
            .map((task) => task.result_summary || task.error || '')
            .find((item) => String(item || '').trim()) || ''
        ).trim();
      const updatedTime = Math.max(
        Number(member?.active_session_total ? mission.updated_time || 0 : 0),
        ...agentTasks.map((task) => Number(task.updated_time || task.finished_time || task.started_time || 0)),
        Number(mission.updated_time || 0)
      );
      const meta: CanvasNodeMeta = {
        id: nodeId,
        agent_id: agentId,
        agent_name: name,
        role: isMother ? 'mother' : 'worker',
        role_label: isMother ? t('beeroom.canvas.legendMother') : t('beeroom.canvas.legendWorker'),
        status,
        task_total: agentTasks.length,
        active_session_total: Number(member?.active_session_total || 0),
        updated_time: updatedTime,
        summary,
        entry_agent: agentId === entryAgentId
      };
      nodeMetaMap.set(nodeId, meta);
      const badge = isMother ? t('beeroom.canvas.legendMother') : t('beeroom.canvas.legendWorker');
      return {
        id: nodeId,
        data: meta,
        style: {
          size: isMother ? [280, 108] : [240, 92],
          radius: 18,
          fill: tone.fill,
          stroke: tone.stroke,
          lineWidth: isMother ? 3 : 2,
          shadowColor: 'rgba(15, 23, 42, 0.08)',
          shadowBlur: 12,
          labelText: `${name}\n${badge} · ${resolveStatusLabel(status)}\n${t('beeroom.canvas.currentTaskTotal', { count: agentTasks.length })}`,
          labelFill: '#0f172a',
          labelFontWeight: isMother ? 700 : 600,
          labelFontSize: 12,
          labelLineHeight: 18,
          labelWordWrap: true,
          labelMaxWidth: '82%'
        }
      };
    });

  const edges: any[] = [];
  Array.from(tasksByAgent.entries()).forEach(([agentId, agentTasks]) => {
    if (!motherAgentId || !agentId || agentId === motherAgentId) return;
    const dispatchTask = [...agentTasks]
      .sort((left, right) => Number(left.started_time || left.updated_time || 0) - Number(right.started_time || right.updated_time || 0))[0];
    edges.push({
      id: `dispatch:${motherAgentId}:${agentId}`,
      source: `agent:${motherAgentId}`,
      target: `agent:${agentId}`,
      style: {
        stroke: '#2563eb',
        lineWidth: 2,
        radius: 16,
        endArrow: true,
        labelText: `${t('beeroom.canvas.legendDispatch')} · ${agentTasks.length}`,
        labelFill: '#2563eb'
      },
      data: {
        kind: 'dispatch',
        task_id: dispatchTask?.task_id || null
      }
    });
    const hasReport = agentTasks.some((task) => {
      const status = String(task.status || '').trim().toLowerCase();
      return ['success', 'completed', 'failed', 'error', 'timeout', 'cancelled'].includes(status);
    });
    if (hasReport) {
      edges.push({
        id: `report:${agentId}:${motherAgentId}`,
        source: `agent:${agentId}`,
        target: `agent:${motherAgentId}`,
        style: {
          stroke: '#0f766e',
          lineDash: [6, 6],
          lineWidth: 1.8,
          radius: 16,
          endArrow: true,
          labelText: t('beeroom.canvas.legendReport'),
          labelFill: '#0f766e'
        },
        data: {
          kind: 'report'
        }
      });
    }
  });

  const timeline: TimelineEvent[] = [];
  if (mission.started_time) {
    timeline.push({
      id: `mission:start:${mission.mission_id}`,
      time: Number(mission.started_time || 0),
      title: resolveStatusLabel(missionStatus || 'running'),
      description: `${t('beeroom.summary.motherAgent')}：${nodeMetaMap.get(`agent:${motherAgentId}`)?.agent_name || motherAgentId || '-'}`
    });
  }
  tasks.forEach((task) => {
    const agentName = nodeMetaMap.get(`agent:${task.agent_id}`)?.agent_name || String(task.agent_id || '-');
    const startTime = Number(task.started_time || task.updated_time || 0);
    if (startTime > 0) {
      timeline.push({
        id: `dispatch:${task.task_id}`,
        time: startTime,
        title: t('beeroom.canvas.legendDispatch'),
        description: `${agentName} · ${resolveStatusLabel(task.status || 'running')}`
      });
    }
    const endTime = Number(task.finished_time || 0);
    if (endTime > 0) {
      timeline.push({
        id: `report:${task.task_id}`,
        time: endTime,
        title: t('beeroom.canvas.legendReport'),
        description: `${agentName} · ${String(task.result_summary || task.error || resolveStatusLabel(task.status)).trim()}`
      });
    }
  });
  if (mission.completion_status === 'awaiting_idle') {
    timeline.push({
      id: `awaiting-idle:${mission.mission_id}`,
      time: Number(mission.updated_time || mission.finished_time || mission.started_time || 0),
      title: t('beeroom.canvas.legendAwaitingIdle'),
      description: resolveStatusLabel(mission.completion_status)
    });
  }
  timeline.sort((left, right) => right.time - left.time);

  return {
    nodes,
    edges,
    nodeMetaMap,
    motherNodeId: motherAgentId ? `agent:${motherAgentId}` : '',
    timeline
  };
});

const activeNodeMeta = computed(() => projection.value.nodeMetaMap.get(activeNodeId.value) || null);
const timeline = computed(() => projection.value.timeline.slice(0, 12));

const missionOverview = computed(() => {
  const mission = props.mission;
  if (!mission) return [] as Array<{ key: string; label: string; value: string }>;

  const contextTotal = Number(mission.context_tokens_total || 0);
  const contextPeak = Number(mission.context_tokens_peak || 0);
  const success = Number(mission.task_success || 0);
  const total = Number(mission.task_total || mission.tasks?.length || 0);

  return [
    {
      key: 'status',
      label: t('beeroom.canvas.missionStatus'),
      value: resolveStatusLabel(mission.completion_status || mission.status)
    },
    {
      key: 'closure',
      label: t('beeroom.canvas.closureState'),
      value: mission.all_agents_idle
        ? t('beeroom.canvas.closureClosed')
        : t('beeroom.canvas.closureOpen')
    },
    {
      key: 'task-progress',
      label: t('beeroom.canvas.taskProgress'),
      value: `${success}/${total || 0}`
    },
    {
      key: 'tokens',
      label: t('beeroom.canvas.contextTokens'),
      value: contextPeak > 0 ? `${contextTotal} / ${contextPeak}` : String(contextTotal)
    },
    {
      key: 'rounds',
      label: t('beeroom.canvas.modelRounds'),
      value: String(Number(mission.model_round_total || 0))
    },
    {
      key: 'updated',
      label: t('beeroom.canvas.lastUpdate'),
      value: formatDateTime(mission.updated_time || mission.finished_time || mission.started_time)
    }
  ];
});

const renderSignature = computed(() => {
  const mission = props.mission;
  if (!mission) return '';
  const tasksSignature = (Array.isArray(mission.tasks) ? mission.tasks : [])
    .map(
      (task) =>
        `${task.task_id}:${task.agent_id}:${task.status || ''}:${task.updated_time || 0}:${task.finished_time || 0}`
    )
    .join('|');
  const agentsSignature = (Array.isArray(props.agents) ? props.agents : [])
    .map(
      (agent) =>
        `${agent.agent_id}:${agent.idle === false ? 'busy' : 'idle'}:${agent.active_session_total || 0}`
    )
    .join('|');
  return [
    mission.mission_id || mission.team_run_id || '',
    mission.status || '',
    mission.completion_status || '',
    mission.updated_time || 0,
    mission.finished_time || 0,
    tasksSignature,
    agentsSignature
  ].join('||');
});

const layoutSignature = computed(() => {
  const mission = props.mission;
  if (!mission) return '';
  return `${mission.mission_id || mission.team_run_id || ''}:${projection.value.nodes.length}:${projection.value.edges.length}`;
});

let lastLayoutSignature = '';
let lastMissionIdentity = '';

const ensureGraph = async () => {
  if (!canvasRef.value || graph) return;
  graph = new Graph({
    container: canvasRef.value,
    width: Math.max(320, canvasRef.value.clientWidth || 320),
    height: Math.max(320, canvasRef.value.clientHeight || 320),
    data: { nodes: [], edges: [] },
    node: {
      type: 'rect',
      style: {
        labelPlacement: 'center'
      }
    },
    edge: {
      type: 'polyline'
    },
    layout: {
      type: 'dagre',
      rankdir: 'TB',
      nodesep: 36,
      ranksep: 76
    },
    behaviors: ['drag-canvas', 'zoom-canvas', 'drag-element', 'hover-activate'],
    transforms: ['process-parallel-edges'],
    animation: false,
    theme: 'light'
  });

  graph.on('node:click', (event: any) => {
    const nodeId = String(event?.target?.id || '').trim();
    if (!nodeId) return;
    activeNodeId.value = nodeId;
  });

  graph.on('node:dblclick', (event: any) => {
    const nodeId = String(event?.target?.id || '').trim();
    if (!nodeId) return;
    const meta = projection.value.nodeMetaMap.get(nodeId);
    if (meta?.agent_id) {
      emit('open-agent', meta.agent_id);
    }
  });

  resizeObserver = new ResizeObserver(() => {
    if (!graph || !canvasRef.value) return;
    graph.resize(
      Math.max(320, canvasRef.value.clientWidth || 320),
      Math.max(320, canvasRef.value.clientHeight || 320)
    );
  });
  resizeObserver.observe(canvasRef.value);
};

const clearGraph = async () => {
  if (!graph) return;
  graph.setData({ nodes: [], edges: [] });
  await graph.render();
};

const renderGraph = async (forceFit = false) => {
  if (!props.mission) {
    activeNodeId.value = '';
    await clearGraph();
    lastLayoutSignature = '';
    return;
  }
  await nextTick();
  await ensureGraph();
  if (!graph) return;
  graph.setData({
    nodes: projection.value.nodes,
    edges: projection.value.edges
  });
  await graph.render();
  if (forceFit || lastLayoutSignature !== layoutSignature.value) {
    await graph.fitView();
    lastLayoutSignature = layoutSignature.value;
  }
};

const fitCanvas = async () => {
  if (!graph) return;
  await graph.fitView();
};

const focusMother = async () => {
  if (!graph || !projection.value.motherNodeId) return;
  activeNodeId.value = projection.value.motherNodeId;
  await graph.focusElement(projection.value.motherNodeId);
};

watch(
  renderSignature,
  async () => {
    const missionIdentity = String(props.mission?.mission_id || props.mission?.team_run_id || '').trim();
    const missionChanged = missionIdentity !== lastMissionIdentity;
    lastMissionIdentity = missionIdentity;

    if (!activeNodeId.value || !projection.value.nodeMetaMap.has(activeNodeId.value) || missionChanged) {
      activeNodeId.value = projection.value.motherNodeId;
    }

    await renderGraph(missionChanged);
  },
  { immediate: true }
);

onMounted(async () => {
  await renderGraph();
});

onBeforeUnmount(() => {
  resizeObserver?.disconnect();
  resizeObserver = null;
  graph?.destroy();
  graph = null;
});
</script>

<style scoped>
.beeroom-panel--canvas {
  overflow: hidden;
}

.beeroom-canvas-actions {
  display: inline-flex;
  gap: 10px;
  flex-wrap: wrap;
}

.beeroom-canvas-layout {
  display: grid;
  grid-template-columns: minmax(0, 1.4fr) 320px;
  gap: 16px;
}

.beeroom-canvas-surface-wrap,
.beeroom-canvas-card {
  border: 1px solid var(--hula-border);
  border-radius: 16px;
  background: var(--hula-main-bg);
}

.beeroom-canvas-surface-wrap {
  display: flex;
  min-height: 520px;
  flex-direction: column;
  overflow: hidden;
}

.beeroom-canvas-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  padding: 12px 14px;
  border-bottom: 1px solid var(--hula-border);
  color: var(--hula-muted);
  font-size: 12px;
}

.beeroom-canvas-legend-item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.beeroom-canvas-legend-item .dot {
  width: 10px;
  height: 10px;
  border-radius: 999px;
}

.beeroom-canvas-legend-item .dot.mother {
  background: #f59e0b;
}

.beeroom-canvas-legend-item .dot.worker {
  background: #3b82f6;
}

.beeroom-canvas-legend-item .line {
  width: 18px;
  height: 0;
  border-top: 2px solid #3b82f6;
}

.beeroom-canvas-legend-item .line.report {
  border-top-style: dashed;
  border-top-color: #0f766e;
}

.beeroom-canvas-legend-item .chip.awaiting {
  width: 12px;
  height: 12px;
  border-radius: 4px;
  background: #fef3c7;
  border: 1px solid #d97706;
}

.beeroom-canvas-surface {
  flex: 1;
  min-height: 460px;
}

.beeroom-canvas-sidebar {
  display: grid;
  gap: 14px;
  align-content: start;
}

.beeroom-canvas-card {
  padding: 14px;
}

.beeroom-canvas-overview-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.beeroom-canvas-overview-item {
  padding: 12px;
  border-radius: 12px;
  background: var(--hula-soft-bg);
}

.beeroom-canvas-overview-label {
  color: var(--hula-muted);
  font-size: 12px;
}

.beeroom-canvas-overview-value {
  margin-top: 6px;
  color: var(--hula-text-color);
  font-size: 14px;
  font-weight: 700;
  line-height: 1.4;
}

.beeroom-canvas-card-title {
  margin-bottom: 10px;
  font-size: 14px;
  font-weight: 700;
}

.beeroom-canvas-node-name-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.beeroom-canvas-node-name {
  font-size: 16px;
  font-weight: 700;
}

.beeroom-canvas-node-role,
.beeroom-canvas-node-hint,
.beeroom-canvas-timeline-time,
.beeroom-canvas-node-empty {
  color: var(--hula-muted);
  font-size: 12px;
}

.beeroom-inline-link {
  padding: 0;
  border: none;
  background: transparent;
  color: var(--hula-accent);
  cursor: pointer;
}

.beeroom-canvas-node-grid {
  display: grid;
  gap: 8px;
  margin-top: 12px;
}

.beeroom-canvas-node-field {
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--hula-soft-bg);
}

.beeroom-canvas-node-summary {
  margin-top: 12px;
  line-height: 1.6;
}

.beeroom-canvas-node-hint {
  margin-top: 8px;
}

.beeroom-canvas-timeline {
  display: grid;
  gap: 10px;
  max-height: 340px;
  overflow: auto;
}

.beeroom-canvas-timeline-item {
  display: grid;
  grid-template-columns: 74px minmax(0, 1fr);
  gap: 10px;
  align-items: start;
}

.beeroom-canvas-timeline-title {
  font-weight: 600;
}

.beeroom-canvas-timeline-desc {
  margin-top: 4px;
  color: var(--hula-text-color);
  line-height: 1.5;
}

@media (max-width: 1200px) {
  .beeroom-canvas-layout {
    grid-template-columns: 1fr;
  }

  .beeroom-canvas-sidebar {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 860px) {
  .beeroom-canvas-overview-grid {
    grid-template-columns: 1fr;
  }

  .beeroom-canvas-sidebar {
    grid-template-columns: 1fr;
  }
}
</style>
