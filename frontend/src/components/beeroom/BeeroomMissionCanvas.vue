<template>
  <section class="beeroom-canvas-screen" :class="{ 'is-empty': !projection.nodes.length }">
    <div v-if="!projection.nodes.length" class="beeroom-canvas-empty">
      <i class="fa-solid fa-diagram-project" aria-hidden="true"></i>
      <span>{{ t('beeroom.canvas.empty') }}</span>
    </div>

    <div v-else class="beeroom-canvas-layout">
      <div class="beeroom-canvas-board" :class="{ 'chat-collapsed': chatCollapsed }">
        <div ref="boardRef" class="beeroom-canvas-graph-shell">
          <div ref="canvasRef" class="beeroom-canvas-surface"></div>
          <div class="beeroom-canvas-minimap-shell">
            <div class="beeroom-canvas-minimap-label">{{ t('beeroom.canvas.minimap') }}</div>
            <div ref="minimapRef" class="beeroom-canvas-minimap"></div>
          </div>

          <div
            v-if="hoveredNodeMeta"
            class="beeroom-canvas-tooltip"
            :style="hoveredTooltipStyle"
          >
            <div class="beeroom-canvas-tooltip-head">
              <div class="beeroom-canvas-tooltip-title">{{ hoveredNodeMeta.agent_name }}</div>
              <span class="beeroom-canvas-role-chip">{{ hoveredNodeMeta.role_label }}</span>
            </div>
            <div class="beeroom-canvas-tooltip-meta">
              <span class="beeroom-canvas-status-chip" :class="resolveToneClass(hoveredNodeMeta.status)">
                {{ resolveStatusLabel(hoveredNodeMeta.status) }}
              </span>
              <span v-if="hoveredNodeMeta.entry_agent" class="beeroom-canvas-entry-flag">
                {{ t('beeroom.canvas.entryAgent') }}
              </span>
            </div>
            <div class="beeroom-canvas-tooltip-grid">
              <div class="beeroom-canvas-tooltip-item">
                <span>{{ t('beeroom.canvas.currentTaskTotal', { count: hoveredNodeMeta.task_total || 0 }) }}</span>
                <strong>{{ hoveredNodeMeta.task_total || 0 }}</strong>
              </div>
              <div class="beeroom-canvas-tooltip-item">
                <span>{{ t('beeroom.canvas.activeSessions') }}</span>
                <strong>{{ hoveredNodeMeta.active_session_total || 0 }}</strong>
              </div>
            </div>
            <p class="beeroom-canvas-tooltip-desc">{{ hoveredNodeDescription }}</p>
          </div>
        </div>

        <aside class="beeroom-canvas-chat" :class="{ collapsed: chatCollapsed }">
        <button
          class="beeroom-canvas-chat-handle"
          type="button"
          :title="chatCollapsed ? t('common.expand') : t('common.collapse')"
          :aria-label="chatCollapsed ? t('common.expand') : t('common.collapse')"
          @click="chatCollapsed = !chatCollapsed"
        >
          <i class="fa-solid" :class="chatCollapsed ? 'fa-chevron-left' : 'fa-chevron-right'" aria-hidden="true"></i>
        </button>
        <template v-if="!chatCollapsed">
          <div class="beeroom-canvas-chat-head">
            <div>
              <div class="beeroom-canvas-chat-title">{{ t('beeroom.canvas.chatTitle') }}</div>
              <div class="beeroom-canvas-chat-subtitle">{{ chatPanelSubtitle }}</div>
            </div>
            <div class="beeroom-canvas-chat-head-actions">
              <button
                class="beeroom-canvas-icon-btn"
                type="button"
                :title="t('common.clear')"
                @click="clearManualChatHistory"
              >
                <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
              </button>
              <button
                class="beeroom-canvas-icon-btn"
                type="button"
                :title="refreshing ? t('common.loading') : t('common.refresh')"
                :disabled="refreshing"
                @click="emit('refresh')"
              >
                <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
              </button>
              <span class="beeroom-canvas-chat-count">{{ displayChatMessages.length }}</span>
            </div>
          </div>

          <section ref="chatStreamRef" class="beeroom-canvas-chat-stream">
            <article
              v-for="message in displayChatMessages"
              :key="message.key"
              class="beeroom-canvas-chat-message"
              :class="[`is-${message.tone}`]"
            >
              <button
                v-if="message.senderAgentId"
                class="beeroom-canvas-chat-avatar"
                type="button"
                @click="emit('open-agent', message.senderAgentId)"
              >
                {{ avatarLabel(message.senderName) }}
              </button>
              <div v-else class="beeroom-canvas-chat-avatar" :class="message.tone === 'user' ? 'is-user' : 'is-system'">
                <i
                  class="fa-solid"
                  :class="message.tone === 'user' ? 'fa-user' : 'fa-wave-square'"
                  aria-hidden="true"
                ></i>
              </div>
              <div class="beeroom-canvas-chat-main">
                <div class="beeroom-canvas-chat-meta-row">
                  <button
                    v-if="message.senderAgentId"
                    class="beeroom-canvas-chat-sender"
                    type="button"
                    @click="emit('open-agent', message.senderAgentId)"
                  >
                    {{ message.senderName }}
                  </button>
                  <span
                    v-else
                    class="beeroom-canvas-chat-sender"
                    :class="message.tone === 'user' ? 'is-user' : 'is-system'"
                  >
                    {{ message.senderName }}
                  </span>
                  <span class="beeroom-canvas-chat-time">{{ message.timeLabel }}</span>
                </div>
                <div class="beeroom-canvas-chat-bubble">
                  <span v-if="message.mention" class="beeroom-canvas-chat-mention">@{{ message.mention }}</span>
                  <span>{{ message.body }}</span>
                </div>
                <div v-if="message.meta" class="beeroom-canvas-chat-extra">{{ message.meta }}</div>
              </div>
            </article>
          </section>

          <section class="beeroom-canvas-chat-composer">
            <textarea
              v-model="composerText"
              class="beeroom-canvas-chat-textarea"
              :placeholder="t('beeroom.canvas.chatInputPlaceholder')"
              :disabled="composerSending"
              rows="3"
              @keydown.enter.exact.prevent="handleComposerSend"
            ></textarea>
            <div class="beeroom-canvas-chat-compose-foot">
              <el-select
                id="beeroom-chat-target"
                v-model="composerTargetAgentId"
                class="beeroom-canvas-chat-select"
                popper-class="beeroom-canvas-chat-select-popper"
                :placeholder="t('beeroom.canvas.chatTarget')"
                :disabled="composerSending"
              >
                <el-option
                  v-for="option in composerTargetOptions"
                  :key="option.agentId"
                  :label="option.label"
                  :value="option.agentId"
                />
              </el-select>
              <button
                class="beeroom-canvas-chat-send"
                type="button"
                :disabled="composerSending || !composerCanSend"
                @click="handleComposerSend"
              >
                {{ composerSending ? t('common.loading') : t('chat.input.send') }}
              </button>
            </div>
            <div v-if="composerError" class="beeroom-canvas-chat-compose-status is-error">{{ composerError }}</div>
          </section>
        </template>
        </aside>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { Graph } from '@antv/g6';
import { ElMessage } from 'element-plus';
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import {
  appendBeeroomChatMessage,
  clearBeeroomChatMessages,
  listBeeroomChatMessages
} from '@/api/beeroom';
import { createSession, listSessions, sendMessageStream } from '@/api/chat';
import { useI18n } from '@/i18n';
import { consumeSseStream } from '@/utils/sse';
import { DEFAULT_AGENT_KEY } from '@/views/messenger/model';
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

type CanvasLinkItem = {
  key: string;
  title: string;
  subtitle: string;
  meta: string;
};

type MissionChatMessage = {
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

type ComposerTargetOption = {
  agentId: string;
  label: string;
  role: 'mother' | 'worker';
};

type CanvasPositionOverride = {
  x: number;
  y: number;
};

type HoneycombSlot = {
  q: number;
  r: number;
};

const HEX_DIRECTIONS: HoneycombSlot[] = [
  { q: 1, r: 0 },
  { q: 1, r: -1 },
  { q: 0, r: -1 },
  { q: -1, r: 0 },
  { q: -1, r: 1 },
  { q: 0, r: 1 }
];
const HONEYCOMB_RADIUS = 158;
const HONEYCOMB_VERTICAL_RATIO = 1.18;
const NODE_WIDTH = 248;
const MOTHER_NODE_WIDTH = 296;
const NODE_HEIGHT = 96;
const MOTHER_NODE_HEIGHT = 112;
const GRID_PLUGIN_KEY = 'beeroom-grid-line';
const MANUAL_CHAT_HISTORY_LIMIT = 120;
const CHAT_POLL_INTERVAL_MS = 8000;

const props = defineProps<{
  group: BeeroomGroup | null;
  mission: BeeroomMission | null;
  agents: BeeroomMember[];
  refreshing?: boolean;
}>();

const emit = defineEmits<{
  (event: 'open-agent', agentId: string): void;
  (event: 'refresh'): void;
}>();

const { t } = useI18n();
const boardRef = ref<HTMLDivElement | null>(null);
const canvasRef = ref<HTMLDivElement | null>(null);
const minimapRef = ref<HTMLDivElement | null>(null);
const chatStreamRef = ref<HTMLElement | null>(null);
const activeNodeId = ref('');
const hoveredNodeId = ref('');
const hoveredTooltipPosition = ref({ x: 0, y: 0 });
const chatCollapsed = ref(false);
const manualChatMessages = ref<MissionChatMessage[]>([]);
const composerText = ref('');
const composerTargetAgentId = ref('');
const composerSending = ref(false);
const composerError = ref('');
const nodePositionOverrides = ref<Record<string, CanvasPositionOverride>>({});

let manualMessageSerial = 0;

let graph: Graph | null = null;
let resizeObserver: ResizeObserver | null = null;
let resizeFrame = 0;
let chatPollTimer: number | null = null;

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

const formatCount = (value: unknown) => {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric) || numeric < 0) return '-';
  return new Intl.NumberFormat().format(Math.round(numeric));
};

const normalizeManualChatTone = (value: unknown): MissionChatMessage['tone'] => {
  const tone = String(value || '').trim().toLowerCase();
  if (tone === 'mother' || tone === 'worker' || tone === 'system' || tone === 'user') {
    return tone;
  }
  return 'system';
};

const mapApiChatMessage = (value: unknown): MissionChatMessage | null => {
  if (!value || typeof value !== 'object') return null;
  const payload = value as Record<string, unknown>;
  const messageId = Number(payload.message_id || 0);
  const time = Number(payload.created_at || payload.time || 0);
  if (!Number.isFinite(time) || time <= 0) return null;
  const body = String(payload.body || '').trim();
  if (!body) return null;
  const key = String(payload.key || '').trim() || `history:${messageId || time}`;
  return {
    key,
    senderName: String(payload.sender_name || payload.senderName || '').trim() || t('messenger.section.swarms'),
    senderAgentId: String(payload.sender_agent_id || payload.senderAgentId || '').trim(),
    mention: String(payload.mention_name || payload.mention || payload.mentionName || '').trim(),
    body,
    meta: String(payload.meta || '').trim(),
    time,
    timeLabel: formatDateTime(time),
    tone: normalizeManualChatTone(payload.tone)
  };
};

const shortIdentity = (value: unknown, head = 8, tail = 6) => {
  const text = String(value || '').trim();
  if (!text) return '-';
  if (text.length <= head + tail + 3) return text;
  return `${text.slice(0, head)}...${text.slice(-tail)}`;
};

const avatarLabel = (value: unknown) => String(value || '?').trim().slice(0, 1).toUpperCase() || '?';

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

const resolveToneClass = (value: unknown) => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'running' || normalized === 'queued') return 'tone-running';
  if (normalized === 'completed' || normalized === 'success') return 'tone-success';
  if (normalized === 'failed' || normalized === 'error' || normalized === 'timeout') return 'tone-danger';
  if (normalized === 'awaiting_idle') return 'tone-warn';
  return 'tone-muted';
};

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

const resolveHoneycombPosition = (slot: HoneycombSlot) => {
  const x = HONEYCOMB_RADIUS * Math.sqrt(3) * (slot.q + slot.r / 2);
  const y = HONEYCOMB_RADIUS * HONEYCOMB_VERTICAL_RATIO * slot.r;
  return {
    x: Math.round(x),
    y: Math.round(y)
  };
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

const projection = computed(() => {
  const mission = props.mission;
  const tasks = Array.isArray(mission?.tasks) ? mission.tasks : [];
  const members = Array.isArray(props.agents) ? props.agents : [];
  const memberMap = new Map(members.map((agent) => [String(agent.agent_id || '').trim(), agent]));
  const motherAgentId = String(
    mission?.mother_agent_id ||
      props.group?.mother_agent_id ||
      mission?.entry_agent_id ||
      members[0]?.agent_id ||
      ''
  ).trim();
  const entryAgentId = String(mission?.entry_agent_id || '').trim();
  const missionStatus = String(mission?.completion_status || mission?.status || '').trim().toLowerCase() || 'idle';
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
      nodes: [] as any[],
      edges: [] as any[],
      nodeMetaMap: new Map<string, CanvasNodeMeta>(),
      memberMap,
      tasksByAgent: new Map<string, BeeroomMissionTask[]>(),
      motherNodeId: '',
      extent: { width: 0, height: 0 }
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
  let minX = 0;
  let maxX = 0;
  let minY = 0;
  let maxY = 0;

  const nodes = orderedAgentIds.map((agentId, index) => {
    const member = memberMap.get(agentId);
    const agentTasks = tasksByAgent.get(agentId) || [];
    const isMother = agentId === motherAgentId || (!motherAgentId && index === 0);
    const status = resolveNodeStatus(agentTasks, member, missionStatus);
    const nodeId = `agent:${agentId}`;
    const badge = isMother ? t('beeroom.canvas.legendMother') : t('beeroom.canvas.legendWorker');
    const name = String(member?.name || (isMother ? props.group?.mother_agent_name : '') || agentId);
    const summary = String(
      agentTasks
        .map((task) => task.result_summary || task.error || '')
        .find((item) => String(item || '').trim()) || member?.description || ''
    ).trim();
    const detailLabel = agentTasks.length > 0
      ? `${t('beeroom.canvas.currentTaskTotal', { count: agentTasks.length })} / ${t('beeroom.members.sessions', {
          count: Number(member?.active_session_total || 0)
        })}`
      : `${t('beeroom.members.idle')} / ${t('beeroom.members.sessions', {
          count: Number(member?.active_session_total || 0)
        })}`;
    const tone =
      status === 'completed'
        ? { fill: 'rgba(6, 95, 70, 0.22)', stroke: 'rgba(52, 211, 153, 0.82)', glow: 'rgba(16, 185, 129, 0.18)' }
        : status === 'failed' || status === 'cancelled'
          ? { fill: 'rgba(127, 29, 29, 0.22)', stroke: 'rgba(248, 113, 113, 0.84)', glow: 'rgba(239, 68, 68, 0.18)' }
          : status === 'awaiting_idle'
            ? { fill: 'rgba(120, 53, 15, 0.2)', stroke: 'rgba(251, 191, 36, 0.86)', glow: 'rgba(245, 158, 11, 0.16)' }
            : status === 'running'
              ? { fill: 'rgba(8, 47, 73, 0.24)', stroke: 'rgba(56, 189, 248, 0.92)', glow: 'rgba(56, 189, 248, 0.22)' }
              : { fill: 'rgba(9, 16, 31, 0.82)', stroke: 'rgba(125, 211, 252, 0.38)', glow: 'rgba(56, 189, 248, 0.12)' };
    const slot = slots[index] || { q: 0, r: 0 };
    const position = nodePositionOverrides.value[nodeId] || resolveHoneycombPosition(slot);
    const width = isMother ? MOTHER_NODE_WIDTH : NODE_WIDTH;
    const height = isMother ? MOTHER_NODE_HEIGHT : NODE_HEIGHT;

    minX = Math.min(minX, position.x - width / 2);
    maxX = Math.max(maxX, position.x + width / 2);
    minY = Math.min(minY, position.y - height / 2);
    maxY = Math.max(maxY, position.y + height / 2);

    const meta: CanvasNodeMeta = {
      id: nodeId,
      agent_id: agentId,
      agent_name: name,
      role: isMother ? 'mother' : 'worker',
      role_label: badge,
      status,
      task_total: agentTasks.length,
      active_session_total: Number(member?.active_session_total || 0),
      updated_time: Math.max(
        ...agentTasks.map((task) => Number(task.updated_time || task.finished_time || task.started_time || 0)),
        Number(mission?.updated_time || 0)
      ),
      summary,
      entry_agent: agentId === entryAgentId
    };
    nodeMetaMap.set(nodeId, meta);

    return {
      id: nodeId,
      data: meta,
      style: {
        x: position.x,
        y: position.y,
        size: isMother ? [MOTHER_NODE_WIDTH, MOTHER_NODE_HEIGHT] : [NODE_WIDTH, NODE_HEIGHT],
        radius: 20,
        fill: tone.fill,
        stroke: isMother ? 'rgba(34, 211, 238, 0.96)' : tone.stroke,
        lineWidth: isMother ? 1.8 : 1.15,
        shadowColor: isMother ? 'rgba(34, 211, 238, 0.22)' : tone.glow,
        shadowBlur: isMother ? 24 : 16,
        shadowOffsetY: 2,
        labelText: `${name}
${badge} / ${resolveStatusLabel(status)}
${detailLabel}`,
        labelFill: isMother ? '#f8fafc' : '#e2e8f0',
        labelFontWeight: isMother ? 700 : 500,
        labelFontSize: isMother ? 13 : 12,
        labelLineHeight: isMother ? 19 : 18,
        labelWordWrap: true,
        labelMaxWidth: isMother ? '82%' : '80%'
      }
    };
  });

  const edges: any[] = [];
  if (mission) {
    Array.from(tasksByAgent.entries()).forEach(([agentId, agentTasks]) => {
      if (!motherAgentId || !agentId || agentId === motherAgentId) return;
      const dispatchTask = [...agentTasks].sort(
        (left, right) => Number(left.started_time || left.updated_time || 0) - Number(right.started_time || right.updated_time || 0)
      )[0];
      edges.push({
        id: `dispatch:${motherAgentId}:${agentId}`,
        source: `agent:${motherAgentId}`,
        target: `agent:${agentId}`,
        style: {
          stroke: 'rgba(59, 130, 246, 0.74)',
          lineWidth: 1.15,
          lineDash: [8, 6],
          radius: 16,
          endArrow: true,
          labelText: `${t('beeroom.canvas.legendDispatch')} / ${agentTasks.length}`,
          labelFill: 'rgba(125, 211, 252, 0.82)',
          labelFontSize: 10
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
            stroke: 'rgba(45, 212, 191, 0.78)',
            lineDash: [4, 8],
            lineWidth: 1.05,
            radius: 16,
            endArrow: true,
            labelText: t('beeroom.canvas.legendReport'),
            labelFill: 'rgba(153, 246, 228, 0.88)',
            labelFontSize: 10
          },
          data: {
            kind: 'report'
          }
        });
      }
    });
  }

  return {
    nodes,
    edges,
    nodeMetaMap,
    memberMap,
    tasksByAgent,
    motherNodeId: motherAgentId ? `agent:${motherAgentId}` : nodes[0]?.id || '',
    extent: {
      width: Math.max(0, maxX - minX),
      height: Math.max(0, maxY - minY)
    }
  };
});

const activeNodeMeta = computed(() => projection.value.nodeMetaMap.get(activeNodeId.value) || null);
const activeNodeMember = computed(() => {
  const meta = activeNodeMeta.value;
  return meta ? projection.value.memberMap.get(meta.agent_id) || null : null;
});
const activeNodeTasks = computed(() => {
  const meta = activeNodeMeta.value;
  if (!meta) return [] as BeeroomMissionTask[];
  return [...(projection.value.tasksByAgent.get(meta.agent_id) || [])]
    .sort(
      (left, right) =>
        Number(right.updated_time || right.finished_time || right.started_time || 0) -
        Number(left.updated_time || left.finished_time || left.started_time || 0)
    )
    .slice(0, 3);
});
const activeNodeDescription = computed(() => {
  const member = activeNodeMember.value;
  const meta = activeNodeMeta.value;
  return String(member?.description || meta?.summary || t('beeroom.members.noDescription'));
});
const activeNodeStats = computed(() => {
  const meta = activeNodeMeta.value;
  if (!meta) return [] as Array<{ key: string; label: string; value: string }>;
  return [
    {
      key: 'status',
      label: t('beeroom.canvas.currentStatus'),
      value: resolveStatusLabel(meta.status)
    },
    {
      key: 'sessions',
      label: t('beeroom.canvas.activeSessions'),
      value: String(meta.active_session_total || 0)
    },
    {
      key: 'tasks',
      label: t('beeroom.canvas.currentTaskTotal', { count: meta.task_total || 0 }),
      value: String(meta.task_total || 0)
    },
    {
      key: 'updated',
      label: t('beeroom.canvas.lastUpdate'),
      value: formatDateTime(meta.updated_time)
    }
  ];
});

const activeNodeContextRows = computed(() => {
  const mission = props.mission;
  return [
    {
      key: 'total',
      label: t('beeroom.canvas.contextTotal'),
      value: mission ? formatCount(mission.context_tokens_total) : '-'
    },
    {
      key: 'peak',
      label: t('beeroom.canvas.contextPeak'),
      value: mission ? formatCount(mission.context_tokens_peak) : '-'
    },
    {
      key: 'rounds',
      label: t('beeroom.canvas.modelRounds'),
      value: mission ? formatCount(mission.model_round_total) : '-'
    }
  ];
});

const activeNodeToolNames = computed(() => {
  const member = activeNodeMember.value;
  const values = Array.isArray(member?.tool_names) ? member.tool_names : [];
  return Array.from(new Set(values.map((item) => String(item || '').trim()).filter(Boolean))).slice(0, 8);
});

const resolveCollaborationMeta = (task: BeeroomMissionTask | null | undefined) => {
  if (!task) return '';
  if (String(task.session_run_id || '').trim()) {
    return `${t('beeroom.task.runId')} ${shortIdentity(task.session_run_id)}`;
  }
  const sessionId = task.spawned_session_id || task.target_session_id || '';
  if (String(sessionId || '').trim()) {
    return `${t('beeroom.task.sessionId')} ${shortIdentity(sessionId)}`;
  }
  return formatDateTime(task.updated_time || task.finished_time || task.started_time || 0);
};

const activeNodeCollaborationLinks = computed(() => {
  const meta = activeNodeMeta.value;
  if (!meta) return [] as CanvasLinkItem[];

  const mission = props.mission;
  const motherMeta = projection.value.nodeMetaMap.get(projection.value.motherNodeId) || null;
  const latestTask = activeNodeTasks.value[0] || null;
  const links: CanvasLinkItem[] = [];

  if (meta.role === 'mother') {
    Array.from(projection.value.tasksByAgent.entries())
      .filter(([agentId, tasks]) => agentId !== meta.agent_id && tasks.length > 0)
      .map(([agentId, tasks]) => {
        const latest = [...tasks].sort(
          (left, right) =>
            Number(right.updated_time || right.finished_time || right.started_time || 0) -
            Number(left.updated_time || left.finished_time || left.started_time || 0)
        )[0];
        const workerMeta = projection.value.nodeMetaMap.get(`agent:${agentId}`);
        return {
          key: `mother:${agentId}:${latest?.task_id || ''}`,
          title: `${meta.agent_name} -> ${workerMeta?.agent_name || agentId}`,
          subtitle: t('beeroom.canvas.legendDispatch'),
          meta: resolveCollaborationMeta(latest),
          updated: Number(latest?.updated_time || latest?.finished_time || latest?.started_time || 0)
        };
      })
      .sort((left, right) => right.updated - left.updated)
      .slice(0, 3)
      .forEach((item) => {
        links.push({ key: item.key, title: item.title, subtitle: item.subtitle, meta: item.meta });
      });
    return links;
  }

  if (motherMeta?.agent_name) {
    links.push({
      key: `dispatch:${meta.agent_id}`,
      title: `${motherMeta.agent_name} -> ${meta.agent_name}`,
      subtitle: t('beeroom.canvas.legendDispatch'),
      meta: resolveCollaborationMeta(latestTask)
    });
  }

  if (latestTask) {
    const terminal = ['success', 'completed', 'failed', 'error', 'timeout', 'cancelled'].includes(
      String(latestTask.status || '').trim().toLowerCase()
    );
    if (terminal && motherMeta?.agent_name) {
      links.push({
        key: `report:${latestTask.task_id}`,
        title: `${meta.agent_name} -> ${motherMeta.agent_name}`,
        subtitle: t('beeroom.canvas.legendReport'),
        meta: String(latestTask.result_summary || latestTask.error || resolveStatusLabel(latestTask.status)).trim()
      });
    }
    const runOrSession = String(
      latestTask.session_run_id || latestTask.spawned_session_id || latestTask.target_session_id || ''
    ).trim();
    if (runOrSession) {
      links.push({
        key: `run:${runOrSession}`,
        title: meta.agent_name,
        subtitle: latestTask.session_run_id ? t('beeroom.task.runId') : t('beeroom.task.sessionId'),
        meta: shortIdentity(runOrSession)
      });
    }
  }

  if (!links.length && mission) {
    links.push({
      key: `mission:${mission.mission_id || mission.team_run_id}`,
      title: meta.agent_name,
      subtitle: t('beeroom.canvas.currentStatus'),
      meta: resolveStatusLabel(meta.status)
    });
  }
  return links.slice(0, 3);
});

const activeNodeToolSummaryText = computed(() => {
  const toolNames = activeNodeToolNames.value;
  if (!toolNames.length) {
    return t('beeroom.canvas.toolSummaryEmpty');
  }
  return t('beeroom.canvas.toolSummaryFallback', { count: toolNames.length });
});

const hoveredNodeMeta = computed(() => projection.value.nodeMetaMap.get(hoveredNodeId.value) || null);
const hoveredNodeMember = computed(() => {
  const meta = hoveredNodeMeta.value;
  return meta ? projection.value.memberMap.get(meta.agent_id) || null : null;
});
const hoveredNodeDescription = computed(() => {
  const member = hoveredNodeMember.value;
  const meta = hoveredNodeMeta.value;
  return String(member?.description || meta?.summary || t('beeroom.members.noDescription'));
});
const hoveredTooltipStyle = computed(() => {
  const boardRect = boardRef.value?.getBoundingClientRect();
  const boardWidth = Math.max(0, Number(boardRect?.width || boardRef.value?.clientWidth || 0));
  const boardHeight = Math.max(0, Number(boardRect?.height || boardRef.value?.clientHeight || 0));
  const tooltipWidth = 260;
  const tooltipHeight = 168;
  const x = Math.max(12, Math.min(boardWidth - tooltipWidth - 12, hoveredTooltipPosition.value.x + 18));
  const y = Math.max(12, Math.min(boardHeight - tooltipHeight - 12, hoveredTooltipPosition.value.y + 18));
  return {
    left: `${Math.round(x)}px`,
    top: `${Math.round(y)}px`
  };
});

const resolveAgentNameById = (agentId: unknown) => {
  const normalized = String(agentId || '').trim();
  if (!normalized) return '-';
  if (normalized === DEFAULT_AGENT_KEY) {
    return t('messenger.defaultAgent');
  }
  const member = projection.value.memberMap.get(normalized);
  if (member?.name) {
    return String(member.name).trim();
  }
  const meta = projection.value.nodeMetaMap.get(`agent:${normalized}`);
  return String(meta?.agent_name || normalized).trim();
};

const normalizeComparableName = (value: unknown) =>
  String(value || '')
    .trim()
    .toLowerCase()
    .replace(/\s+/g, '');

const resolveMemberAgentKey = (member: BeeroomMember) => {
  const raw = String(member.agent_id || '').trim();
  if (raw) return raw;
  return member.hive_id ? DEFAULT_AGENT_KEY : '';
};

const motherAgentId = computed(() =>
  String(
    props.mission?.mother_agent_id ||
      props.group?.mother_agent_id ||
      props.mission?.entry_agent_id ||
      props.agents[0]?.agent_id ||
      ''
  ).trim()
);

const composerTargetOptions = computed<ComposerTargetOption[]>(() => {
  const seen = new Set<string>();
  const motherId = motherAgentId.value;
  const options: ComposerTargetOption[] = [];

  const pushOption = (agentId: string, role: 'mother' | 'worker') => {
    const normalized = String(agentId || '').trim();
    if (!normalized || seen.has(normalized)) return;
    seen.add(normalized);
    const label = role === 'mother'
      ? `${resolveAgentNameById(normalized)} (${t('beeroom.summary.motherAgent')})`
      : resolveAgentNameById(normalized);
    options.push({ agentId: normalized, label, role });
  };

  if (motherId) {
    pushOption(motherId, 'mother');
  }
  props.agents.forEach((member) => {
    const agentId = resolveMemberAgentKey(member);
    if (!agentId || agentId === motherId) return;
    pushOption(agentId, 'worker');
  });
  return options;
});

const composerCanSend = computed(
  () => String(composerText.value || '').trim().length > 0 && !!composerTargetOptions.value.length
);

const activeGroupId = computed(() => String(props.group?.group_id || '').trim());

const nextManualMessageKey = (prefix: string) => {
  manualMessageSerial += 1;
  return `${prefix}:${Date.now()}:${manualMessageSerial}`;
};

const appendManualChatMessage = (message: MissionChatMessage) => {
  manualChatMessages.value = [...manualChatMessages.value, message]
    .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
    .slice(-MANUAL_CHAT_HISTORY_LIMIT);
};

const replaceManualChatMessages = (messages: MissionChatMessage[]) => {
  manualChatMessages.value = [...messages]
    .sort((left, right) => left.time - right.time || left.key.localeCompare(right.key))
    .slice(-MANUAL_CHAT_HISTORY_LIMIT);
};

const loadManualChatHistory = async () => {
  const groupId = activeGroupId.value;
  if (!groupId) {
    manualChatMessages.value = [];
    return;
  }
  try {
    const response = await listBeeroomChatMessages(groupId, { limit: MANUAL_CHAT_HISTORY_LIMIT });
    const items = Array.isArray(response?.data?.data?.items)
      ? response.data.data.items
          .map((item: unknown) => mapApiChatMessage(item))
          .filter((item: MissionChatMessage | null): item is MissionChatMessage => !!item)
      : [];
    replaceManualChatMessages(items);
  } catch {
    manualChatMessages.value = [];
  }
};

const persistManualChatMessage = async (payload: {
  senderKind: string;
  senderName: string;
  senderAgentId?: string;
  mention?: string;
  mentionAgentId?: string;
  body: string;
  meta?: string;
  tone: MissionChatMessage['tone'];
  createdAt?: number;
  clientMsgId?: string;
}) => {
  const groupId = activeGroupId.value;
  if (!groupId) {
    throw new Error(t('common.requestFailed'));
  }
  const response = await appendBeeroomChatMessage(groupId, {
    senderKind: payload.senderKind,
    senderName: payload.senderName,
    senderAgentId: payload.senderAgentId,
    mentionName: payload.mention,
    mentionAgentId: payload.mentionAgentId,
    body: payload.body,
    meta: payload.meta,
    tone: payload.tone,
    createdAt: payload.createdAt,
    clientMsgId: payload.clientMsgId
  });
  const message = mapApiChatMessage(response?.data?.data);
  if (!message) {
    throw new Error(t('common.requestFailed'));
  }
  appendManualChatMessage(message);
  return message;
};

const clearManualChatHistory = async () => {
  composerError.value = '';
  try {
    const groupId = activeGroupId.value;
    if (groupId) {
      await clearBeeroomChatMessages(groupId);
    }
    manualChatMessages.value = [];
  } catch {
    ElMessage.error(t('common.requestFailed'));
  }
};

const clipMessageBody = (value: unknown, limit = 240) => {
  const text = String(value || '').trim();
  if (!text) return '';
  if (text.length <= limit) return text;
  return `${text.slice(0, limit)}...`;
};

const safeJsonParse = (value: unknown) => {
  try {
    return JSON.parse(String(value || '')) as Record<string, any>;
  } catch {
    return null;
  }
};

const extractReplyText = (payload: Record<string, any> | null) => {
  const candidates = [
    payload?.content,
    payload?.reply,
    payload?.message,
    payload?.text,
    payload?.data?.content,
    payload?.data?.reply,
    payload?.data?.message,
    payload?.data?.text,
    payload?.data?.final_reply,
    payload?.final_reply
  ];
  return clipMessageBody(candidates.find((item) => String(item || '').trim()) || '');
};

const extractErrorText = (payload: Record<string, any> | null) => {
  const candidates = [
    payload?.detail,
    payload?.error,
    payload?.message,
    payload?.data?.detail,
    payload?.data?.error,
    payload?.data?.message
  ];
  return String(candidates.find((item) => String(item || '').trim()) || '').trim();
};

const resolveDispatchTarget = (rawContent: string) => {
  const text = String(rawContent || '').trim();
  const explicitMatch = text.match(/^@([^\s]+)\s*(.*)$/);
  const normalizedExplicit = normalizeComparableName(explicitMatch?.[1] || '');
  const explicitTarget = normalizedExplicit
    ? composerTargetOptions.value.find((item) => {
        const optionName = normalizeComparableName(resolveAgentNameById(item.agentId));
        const optionLabel = normalizeComparableName(item.label.replace(/\(.+?\)/g, ''));
        return optionName === normalizedExplicit || optionLabel === normalizedExplicit;
      }) || null
    : null;
  const target = explicitTarget ||
    composerTargetOptions.value.find((item) => item.agentId === composerTargetAgentId.value) ||
    composerTargetOptions.value[0] ||
    null;
  return {
    target,
    body: String(explicitMatch?.[2] || text).trim() || text
  };
};

const ensureDispatchSessionId = async (agentId: string) => {
  const apiAgentId = agentId === DEFAULT_AGENT_KEY ? '' : agentId;
  const { data } = await listSessions({ agent_id: apiAgentId });
  const source = Array.isArray(data?.data?.items) ? data.data.items : [];
  const matched = source
    .filter((item) => {
      const sessionAgentId = String(item?.agent_id || (item?.is_default === true ? DEFAULT_AGENT_KEY : '')).trim();
      return sessionAgentId === agentId;
    })
    .sort((left, right) => {
      const leftTime = Number(left?.updated_at || left?.last_message_at || left?.created_at || 0);
      const rightTime = Number(right?.updated_at || right?.last_message_at || right?.created_at || 0);
      return rightTime - leftTime;
    });
  const primary = matched.find((item) => item?.is_main === true) || matched[0];
  if (primary?.id) {
    return String(primary.id);
  }
  const created = await createSession(agentId === DEFAULT_AGENT_KEY ? {} : { agent_id: agentId });
  return String(created?.data?.data?.id || '');
};

const scrollChatToBottom = async () => {
  await nextTick();
  const element = chatStreamRef.value;
  if (!element) return;
  element.scrollTop = element.scrollHeight;
};

const handleComposerSend = async () => {
  if (composerSending.value) return;
  const content = String(composerText.value || '').trim();
  if (!content) return;

  const { target, body } = resolveDispatchTarget(content);
  if (!target?.agentId) {
    const message = t('beeroom.canvas.chatTargetRequired');
    composerError.value = message;
    ElMessage.warning(message);
    return;
  }

  const targetName = resolveAgentNameById(target.agentId);
  const now = Math.floor(Date.now() / 1000);
  const visibleBody = String(body || content).trim();
  const targetTone = target.role === 'mother' ? 'mother' : 'worker';

  composerSending.value = true;
  composerError.value = '';
  composerText.value = '';

  await persistManualChatMessage({
    senderKind: 'user',
    senderName: t('chat.message.user'),
    mention: targetName,
    mentionAgentId: target.agentId,
    body: visibleBody,
    meta: props.group?.name || props.group?.group_id || '',
    tone: 'user',
    createdAt: now,
    clientMsgId: nextManualMessageKey('user')
  });
  await persistManualChatMessage({
    senderKind: 'system',
    senderName: t('messenger.section.swarms'),
    mention: targetName,
    mentionAgentId: target.agentId,
    body: t('beeroom.canvas.chatDispatchPending'),
    meta: props.group?.group_id || '',
    tone: 'system',
    createdAt: now,
    clientMsgId: nextManualMessageKey('dispatch')
  });
  await scrollChatToBottom();

  try {
    // Send the message in background so the user can stay on the swarm canvas.
    const sessionId = await ensureDispatchSessionId(target.agentId);
    if (!sessionId) {
      throw new Error(t('common.requestFailed'));
    }
    const response = await sendMessageStream(sessionId, { content: visibleBody, stream: true });
    if (!response.ok) {
      const errorText = String(await response.text()).trim();
      throw new Error(errorText || t('common.requestFailed'));
    }

    let finalPayload: Record<string, any> | null = null;
    let streamError = '';
    await consumeSseStream(response, (eventType, dataText) => {
      const payload = safeJsonParse(dataText);
      if (eventType === 'error') {
        streamError = extractErrorText(payload) || t('common.requestFailed');
      } else if (eventType === 'final') {
        finalPayload = payload;
      }
    });

    if (streamError) {
      throw new Error(streamError);
    }

    const replyText = extractReplyText(finalPayload);
    await persistManualChatMessage({
      senderKind: 'agent',
      senderName: targetName,
      senderAgentId: target.agentId,
      body: replyText || t('beeroom.canvas.chatDispatchAccepted'),
      meta: t('beeroom.canvas.chatResultMeta'),
      tone: targetTone,
      createdAt: Math.floor(Date.now() / 1000),
      clientMsgId: nextManualMessageKey('reply')
    });
    await scrollChatToBottom();
    emit('refresh');
  } catch (error: any) {
    const message = String(error?.message || '').trim() || t('common.requestFailed');
    composerError.value = message;
    try {
      await persistManualChatMessage({
        senderKind: 'system',
        senderName: t('messenger.section.swarms'),
        mention: targetName,
        mentionAgentId: target.agentId,
        body: t('beeroom.canvas.chatDispatchFailed'),
        meta: message,
        tone: 'system',
        createdAt: Math.floor(Date.now() / 1000),
        clientMsgId: nextManualMessageKey('error')
      });
    } catch {
      // Keep the original dispatch error visible even if chat persistence fails.
    }
    await scrollChatToBottom();
    ElMessage.error(message);
  } finally {
    composerSending.value = false;
  }
};

const chatPanelSubtitle = computed(() => {
  const mission = props.mission;
  const groupName = props.group?.name || props.group?.group_id || t('messenger.section.swarms');
  if (!mission) {
    return `${groupName} / ${t('beeroom.canvas.chatStandby')}`;
  }
  return `${groupName} / ${resolveStatusLabel(mission.completion_status || mission.status)} / ${t('beeroom.missions.taskCount', {
    count: mission.task_total || 0
  })}`;
});

const missionChatMessages = computed(() => {
  const mission = props.mission;
  const messages: MissionChatMessage[] = [];

  if (!mission) {
    messages.push({
      key: 'standby',
      senderName: t('messenger.section.swarms'),
      senderAgentId: '',
      mention: '',
      body: t('beeroom.canvas.chatStandbyBody', { count: projection.value.nodes.length || props.agents.length || 0 }),
      meta: props.group?.mother_agent_name || props.group?.mother_agent_id
        ? `${t('beeroom.summary.motherAgent')}: ${props.group?.mother_agent_name || props.group?.mother_agent_id}`
        : '',
      time: Number(props.group?.updated_time || props.group?.created_time || 0),
      timeLabel: formatDateTime(props.group?.updated_time || props.group?.created_time || 0),
      tone: 'system'
    });
    return messages;
  }

  const motherAgentId = String(
    mission.mother_agent_id || props.group?.mother_agent_id || mission.entry_agent_id || ''
  ).trim();
  const motherName = resolveAgentNameById(motherAgentId || props.group?.mother_agent_name || '');
  const entryName = resolveAgentNameById(mission.entry_agent_id || motherAgentId || '');
  const kickoffBody = String(mission.summary || mission.strategy || t('beeroom.missions.noSummary')).trim();

  messages.push({
    key: `kickoff:${mission.mission_id || mission.team_run_id}`,
    senderName: entryName,
    senderAgentId: String(mission.entry_agent_id || motherAgentId || '').trim(),
    mention: motherAgentId && String(mission.entry_agent_id || '').trim() !== motherAgentId ? motherName : '',
    body: kickoffBody,
    meta: `${t('beeroom.canvas.currentStatus')}: ${resolveStatusLabel(mission.completion_status || mission.status)}`,
    time: Number(mission.started_time || mission.updated_time || 0),
    timeLabel: formatDateTime(mission.started_time || mission.updated_time || 0),
    tone: 'mother'
  });

  (Array.isArray(mission.tasks) ? mission.tasks : []).forEach((task) => {
    const workerName = resolveAgentNameById(task.agent_id);
    const startedTime = Number(task.started_time || task.updated_time || 0);
    const finishedTime = Number(task.finished_time || task.updated_time || task.started_time || 0);
    const priority = Number(task.priority || 0);
    const taskId = shortTaskId(task.task_id);
    const runMeta = resolveCollaborationMeta(task);

    messages.push({
      key: `dispatch:${task.task_id}`,
      senderName: motherName,
      senderAgentId: motherAgentId,
      mention: workerName,
      body: t('beeroom.canvas.chatDispatchBody', { taskId, priority }),
      meta: runMeta,
      time: startedTime,
      timeLabel: formatDateTime(startedTime),
      tone: 'mother'
    });

    const normalizedStatus = String(task.status || '').trim().toLowerCase();
    if (['queued', 'running', 'awaiting_idle'].includes(normalizedStatus)) {
      messages.push({
        key: `accept:${task.task_id}`,
        senderName: workerName,
        senderAgentId: String(task.agent_id || '').trim(),
        mention: motherName,
        body: t('beeroom.canvas.chatAcceptBody', { taskId }),
        meta: resolveStatusLabel(task.status),
        time: Number(task.updated_time || task.started_time || 0),
        timeLabel: formatDateTime(task.updated_time || task.started_time || 0),
        tone: 'worker'
      });
    }

    if (task.result_summary || task.error || ['success', 'completed', 'failed', 'error', 'timeout', 'cancelled'].includes(normalizedStatus)) {
      messages.push({
        key: `result:${task.task_id}`,
        senderName: workerName,
        senderAgentId: String(task.agent_id || '').trim(),
        mention: motherName,
        body: String(task.result_summary || task.error || resolveStatusLabel(task.status)).trim(),
        meta: runMeta,
        time: finishedTime,
        timeLabel: formatDateTime(finishedTime),
        tone: 'worker'
      });
    }
  });

  return messages.sort((left, right) => left.time - right.time).slice(-24);
});

const displayChatMessages = computed(() =>
  [...missionChatMessages.value, ...manualChatMessages.value].sort(
    (left, right) => left.time - right.time || left.key.localeCompare(right.key)
  )
);

watch(
  composerTargetOptions,
  (options) => {
    if (!options.length) {
      composerTargetAgentId.value = '';
      return;
    }
    if (!options.some((item) => item.agentId === composerTargetAgentId.value)) {
      composerTargetAgentId.value = options[0]?.agentId || '';
    }
  },
  { immediate: true }
);

watch(
  activeGroupId,
  () => {
    composerText.value = '';
    composerError.value = '';
    void loadManualChatHistory();
    restartChatPolling();
  },
  { immediate: true }
);

watch(
  () => [displayChatMessages.value.length, chatCollapsed.value] as const,
  async ([, collapsed]) => {
    if (collapsed) return;
    await scrollChatToBottom();
  },
  { immediate: true }
);

const shortTaskId = (value: unknown) => {
  const text = String(value || '').trim();
  if (!text) return '-';
  if (text.length <= 12) return text;
  return `${text.slice(0, 6)}...${text.slice(-4)}`;
};

const renderSignature = computed(() => {
  const mission = props.mission;
  const agentsSignature = (Array.isArray(props.agents) ? props.agents : [])
    .map((agent) => `${agent.agent_id}:${agent.idle === false ? 'busy' : 'idle'}:${agent.active_session_total || 0}`)
    .join('|');
  if (!mission) {
    return ['standby', props.group?.group_id || '', props.group?.mother_agent_id || '', agentsSignature].join('||');
  }
  const tasksSignature = (Array.isArray(mission.tasks) ? mission.tasks : [])
    .map((task) => `${task.task_id}:${task.agent_id}:${task.status || ''}:${task.updated_time || 0}:${task.finished_time || 0}`)
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

const nodePositionScopeKey = computed(() =>
  String(props.mission?.mission_id || props.mission?.team_run_id || props.group?.group_id || 'standby').trim()
);

const layoutSignature = computed(() => {
  const identity = props.mission?.mission_id || props.mission?.team_run_id || props.group?.group_id || 'standby';
  return [identity, projection.value.nodes.length, projection.value.edges.length, projection.value.extent.width, projection.value.extent.height].join(':');
});

let lastLayoutSignature = '';
let lastMissionIdentity = '';

const persistDraggedNodePositions = (nodeIds?: string[]) => {
  if (!graph) return;
  const ids = (nodeIds || [])
    .map((item) => String(item || '').trim())
    .filter(Boolean);
  if (!ids.length) return;

  const nextOverrides = { ...nodePositionOverrides.value };
  ids.forEach((nodeId) => {
    try {
      const position = graph?.getElementPosition(nodeId) as { x?: number; y?: number } | undefined;
      const x = Number(position?.x);
      const y = Number(position?.y);
      if (Number.isFinite(x) && Number.isFinite(y)) {
        nextOverrides[nodeId] = { x, y };
      }
    } catch {
      return;
    }
  });
  nodePositionOverrides.value = nextOverrides;
};

const getCanvasViewport = () => {
  const rect = canvasRef.value?.getBoundingClientRect();
  return {
    width: Math.floor(rect?.width || canvasRef.value?.clientWidth || 0),
    height: Math.floor(rect?.height || canvasRef.value?.clientHeight || 0)
  };
};

const normalizeCanvasViewport = (viewport: { width: number; height: number }) => ({
  width: Math.max(360, Number(viewport?.width || 0) || 0),
  height: Math.max(520, Number(viewport?.height || 0) || 0)
});

const waitForCanvasFrame = () =>
  new Promise<void>((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  });

function stopChatPolling() {
  if (chatPollTimer !== null) {
    window.clearInterval(chatPollTimer);
    chatPollTimer = null;
  }
}

function restartChatPolling() {
  stopChatPolling();
  if (typeof window === 'undefined' || !activeGroupId.value) return;
  chatPollTimer = window.setInterval(() => {
    void loadManualChatHistory();
  }, CHAT_POLL_INTERVAL_MS);
}

const waitForCanvasViewport = async (attempts = 10) => {
  let viewport = getCanvasViewport();
  for (let index = 0; index < attempts; index += 1) {
    if (viewport.width > 48 && viewport.height > 48) {
      return normalizeCanvasViewport(viewport);
    }
    await waitForCanvasFrame();
    viewport = getCanvasViewport();
  }
  return normalizeCanvasViewport(viewport);
};

const ensureGraph = async () => {
  if (!canvasRef.value || graph) return;
  const viewport = await waitForCanvasViewport();
  const plugins: any[] = [
    {
      key: GRID_PLUGIN_KEY,
      type: 'grid-line',
      size: 32,
      lineWidth: 1,
      stroke: 'rgba(56, 189, 248, 0.18)',
      border: false,
      follow: true
    }
  ];
  if (minimapRef.value) {
    plugins.push({
      key: 'beeroom-minimap',
      type: 'minimap',
      container: minimapRef.value,
      size: [184, 112],
      padding: 10,
      delay: 64,
      shape: 'key',
      maskStyle: {
        border: '1px solid rgba(56, 189, 248, 0.72)',
        background: 'rgba(56, 189, 248, 0.14)',
        borderRadius: '10px'
      }
    } as any);
  }
  graph = new Graph({
    container: canvasRef.value,
    width: viewport.width,
    height: viewport.height,
    devicePixelRatio: Math.max(1, globalThis.devicePixelRatio || 1),
    data: { nodes: [], edges: [] },
    plugins,
    node: {
      type: 'rect',
      style: {
        labelPlacement: 'center'
      }
    },
    edge: {
      type: 'polyline'
    },
    behaviors: [
      {
        type: 'drag-element',
        key: 'drag-node',
        dropEffect: 'none',
        hideEdge: 'none',
        shadow: false,
        enable: (event: any) => event?.targetType === 'node',
        onFinish: (ids: string[]) => persistDraggedNodePositions(ids)
      },
      'drag-canvas',
      'zoom-canvas',
      'hover-activate'
    ],
    transforms: ['process-parallel-edges'],
    animation: false,
    theme: 'light'
  });

  graph.on('node:pointerenter', (event: any) => {
    const nodeId = String(event?.target?.id || '').trim();
    const boardRect = boardRef.value?.getBoundingClientRect();
    if (!nodeId) return;
    hoveredNodeId.value = nodeId;
    if (boardRect) {
      hoveredTooltipPosition.value = {
        x: Number(event?.client?.x || 0) - boardRect.left,
        y: Number(event?.client?.y || 0) - boardRect.top
      };
    }
  });

  graph.on('node:pointermove', (event: any) => {
    const nodeId = String(event?.target?.id || '').trim();
    const boardRect = boardRef.value?.getBoundingClientRect();
    if (!nodeId || !boardRect) return;
    hoveredNodeId.value = nodeId;
    hoveredTooltipPosition.value = {
      x: Number(event?.client?.x || 0) - boardRect.left,
      y: Number(event?.client?.y || 0) - boardRect.top
    };
  });

  graph.on('node:pointerleave', () => {
    hoveredNodeId.value = '';
  });

  graph.on('node:dragstart', () => {
    hoveredNodeId.value = '';
  });

  graph.on('node:dragend', (event: any) => {
    const nodeId = String(event?.target?.id || '').trim();
    if (!nodeId) return;
    persistDraggedNodePositions([nodeId]);
  });

  graph.on('canvas:pointerleave', () => {
    hoveredNodeId.value = '';
  });

  graph.on('node:dblclick', (event: any) => {
    if (event?.targetType !== 'node') return;
    const nodeId = String(event?.target?.id || '').trim();
    if (!nodeId) return;
    const meta = projection.value.nodeMetaMap.get(nodeId);
    if (meta?.agent_id) {
      emit('open-agent', meta.agent_id);
    }
  });

  resizeObserver = new ResizeObserver(() => {
    if (!graph || !canvasRef.value) return;
    if (resizeFrame) {
      cancelAnimationFrame(resizeFrame);
    }
    resizeFrame = requestAnimationFrame(async () => {
      const viewport = await waitForCanvasViewport(3);
      graph?.resize(viewport.width, viewport.height);
      void graph?.fitView({ when: 'always', direction: 'both' });
      resizeFrame = 0;
    });
  });
  resizeObserver.observe(canvasRef.value);
};

const clearGraph = async () => {
  if (!graph) return;
  graph.setData({ nodes: [], edges: [] });
  await graph.render();
};

const renderGraph = async (forceFit = false) => {
  if (!projection.value.nodes.length) {
    activeNodeId.value = '';
    hoveredNodeId.value = '';
    await clearGraph();
    lastLayoutSignature = '';
    return;
  }
  await nextTick();
  const viewport = await waitForCanvasViewport();
  await ensureGraph();
  if (!graph) return;
  graph.resize(viewport.width, viewport.height);
  graph.setData({ nodes: projection.value.nodes, edges: projection.value.edges });
  await graph.render();
  if (forceFit || lastLayoutSignature !== layoutSignature.value) {
    await graph.fitView({ when: 'always', direction: 'both' });
    lastLayoutSignature = layoutSignature.value;
  }
};

watch(
  nodePositionScopeKey,
  () => {
    // Keep manual dragging local to the current mission/group scope.
    nodePositionOverrides.value = {};
  },
  { immediate: true }
);

watch(
  renderSignature,
  async () => {
    const missionIdentity = String(props.mission?.mission_id || props.mission?.team_run_id || '').trim();
    const missionChanged = missionIdentity !== lastMissionIdentity;
    lastMissionIdentity = missionIdentity;
    if (!activeNodeId.value || !projection.value.nodeMetaMap.has(activeNodeId.value) || missionChanged) {
      activeNodeId.value = projection.value.motherNodeId || projection.value.nodes[0]?.id || '';
    }
    await renderGraph(missionChanged);
  },
  { immediate: true }
);

onMounted(async () => {
  restartChatPolling();
  await renderGraph();
});

onBeforeUnmount(() => {
  stopChatPolling();
  if (resizeFrame) {
    cancelAnimationFrame(resizeFrame);
    resizeFrame = 0;
  }
  resizeObserver?.disconnect();
  resizeObserver = null;
  graph?.destroy();
  graph = null;
});
</script>

<style scoped>
.beeroom-canvas-screen {
  position: relative;
  display: flex;
  flex: 1;
  width: 100%;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  border: 1px solid rgba(56, 189, 248, 0.2);
  border-radius: 20px;
  background:
    radial-gradient(circle at top left, rgba(56, 189, 248, 0.16), transparent 28%),
    radial-gradient(circle at bottom right, rgba(45, 212, 191, 0.14), transparent 32%),
    linear-gradient(180deg, rgba(3, 7, 18, 0.985), rgba(5, 16, 37, 0.975));
  box-shadow:
    0 18px 48px rgba(2, 6, 23, 0.32),
    inset 0 0 0 1px rgba(125, 211, 252, 0.06),
    inset 0 1px 0 rgba(186, 230, 253, 0.04);
}

.beeroom-canvas-screen::before {
  content: '';
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(rgba(56, 189, 248, 0.16) 1px, transparent 1px),
    linear-gradient(90deg, rgba(56, 189, 248, 0.16) 1px, transparent 1px),
    linear-gradient(rgba(14, 165, 233, 0.2) 1px, transparent 1px),
    linear-gradient(90deg, rgba(14, 165, 233, 0.2) 1px, transparent 1px);
  background-size: 28px 28px, 28px 28px, 140px 140px, 140px 140px;
  background-position: 0 0, 0 0, -1px -1px, -1px -1px;
  opacity: 0.58;
  pointer-events: none;
}

.beeroom-canvas-screen::after {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: inherit;
  border: 1px solid rgba(148, 163, 184, 0.08);
  box-shadow:
    inset 0 0 0 1px rgba(56, 189, 248, 0.05),
    inset 0 0 48px rgba(14, 165, 233, 0.05);
  pointer-events: none;
}

.beeroom-canvas-screen.is-empty::after {
  border-style: dashed;
}

.beeroom-canvas-layout {
  position: relative;
  z-index: 1;
  display: flex;
  flex: 1;
  min-height: 0;
  padding: 0;
}

.beeroom-canvas-board {
  --beeroom-chat-width: 344px;
  position: relative;
  display: grid;
  grid-template-columns: minmax(0, 1fr) var(--beeroom-chat-width);
  flex: 1;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  border-radius: inherit;
  overflow: hidden;
  background:
    linear-gradient(180deg, rgba(4, 10, 24, 0.84), rgba(2, 8, 20, 0.68)),
    linear-gradient(90deg, rgba(8, 47, 73, 0.12), transparent 32%, transparent 68%, rgba(8, 47, 73, 0.1));
  box-shadow:
    inset 0 0 0 1px rgba(56, 189, 248, 0.08),
    inset 0 1px 0 rgba(186, 230, 253, 0.04),
    0 16px 34px rgba(2, 6, 23, 0.24);
  transition: grid-template-columns 0.22s ease;
}

.beeroom-canvas-board::before {
  content: '';
  position: absolute;
  inset: 0;
  background:
    linear-gradient(180deg, rgba(56, 189, 248, 0.08), transparent 64px),
    radial-gradient(circle at right center, rgba(34, 211, 238, 0.1), transparent 22%);
  pointer-events: none;
}

.beeroom-canvas-board::after {
  content: '';
  position: absolute;
  top: 0;
  bottom: 0;
  right: var(--beeroom-chat-width);
  width: 1px;
  background: linear-gradient(180deg, transparent, rgba(56, 189, 248, 0.22), transparent);
  pointer-events: none;
  transition: right 0.22s ease;
}

.beeroom-canvas-board.chat-collapsed {
  grid-template-columns: minmax(0, 1fr) 0px;
}

.beeroom-canvas-board.chat-collapsed::after {
  right: 0;
  opacity: 0;
}

.beeroom-canvas-graph-shell {
  position: relative;
  display: flex;
  min-width: 0;
  min-height: 0;
}

.beeroom-canvas-graph-shell::before {
  content: '';
  position: absolute;
  inset: 0;
  border-radius: 0;
  border: 1px solid rgba(56, 189, 248, 0.08);
  box-shadow: inset 0 0 24px rgba(8, 47, 73, 0.12);
  pointer-events: none;
}

.beeroom-canvas-graph-shell::after {
  content: '';
  position: absolute;
  inset: 0;
  background:
    linear-gradient(180deg, rgba(56, 189, 248, 0.06), transparent 92px),
    linear-gradient(135deg, transparent 0%, rgba(56, 189, 248, 0.035) 48%, transparent 100%);
  opacity: 0.5;
  pointer-events: none;
}

.beeroom-canvas-tooltip {
  position: absolute;
  z-index: 6;
  width: 260px;
  display: grid;
  gap: 8px;
  padding: 12px;
  border-radius: 14px;
  border: 1px solid rgba(56, 189, 248, 0.22);
  background: rgba(8, 15, 32, 0.94);
  box-shadow: 0 14px 36px rgba(2, 6, 23, 0.42);
  pointer-events: none;
}

.beeroom-canvas-tooltip-head,
.beeroom-canvas-tooltip-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.beeroom-canvas-tooltip-title {
  color: #f8fafc;
  font-size: 14px;
  font-weight: 700;
}

.beeroom-canvas-tooltip-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.beeroom-canvas-tooltip-item {
  display: grid;
  gap: 4px;
  padding: 8px 10px;
  border-radius: 10px;
  background: rgba(15, 23, 42, 0.78);
  border: 1px solid rgba(148, 163, 184, 0.12);
}

.beeroom-canvas-tooltip-item span,
.beeroom-canvas-tooltip-desc {
  color: rgba(191, 219, 254, 0.76);
  font-size: 11px;
  line-height: 1.55;
}

.beeroom-canvas-tooltip-item strong {
  color: #f8fafc;
  font-size: 13px;
}

.beeroom-canvas-chat {
  position: relative;
  z-index: 1;
  display: flex;
  width: var(--beeroom-chat-width);
  min-width: 0;
  flex-direction: column;
  gap: 12px;
  padding: 14px 14px 14px 18px;
  border-left: 1px solid rgba(56, 189, 248, 0.12);
  background:
    linear-gradient(180deg, rgba(8, 15, 32, 0.92), rgba(4, 10, 24, 0.96)),
    linear-gradient(180deg, rgba(14, 165, 233, 0.06), rgba(45, 212, 191, 0.04));
  color: #e2e8f0;
  box-shadow:
    inset 1px 0 0 rgba(148, 163, 184, 0.04),
    inset 0 1px 0 rgba(186, 230, 253, 0.03);
  overflow: hidden;
  transition: width 0.2s ease, padding 0.2s ease, background 0.2s ease, opacity 0.2s ease;
}

.beeroom-canvas-chat::before {
  content: '';
  position: absolute;
  inset: 0 0 auto 0;
  height: 56px;
  background: linear-gradient(180deg, rgba(56, 189, 248, 0.08), transparent);
  pointer-events: none;
}

.beeroom-canvas-chat.collapsed {
  width: 0;
  padding: 0;
  border-left: 0;
  box-shadow: none;
  background: transparent;
  gap: 0;
  overflow: visible;
}

.beeroom-canvas-chat-handle {
  position: absolute;
  left: -10px;
  top: 50%;
  transform: translateY(-50%);
  width: 18px;
  height: 74px;
  border: 0;
  border-radius: 999px;
  background: linear-gradient(180deg, rgba(69, 203, 255, 0.34), rgba(56, 189, 248, 0.68), rgba(37, 99, 235, 0.58));
  color: #d8ecff;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  z-index: 2;
  opacity: 0;
  pointer-events: none;
  box-shadow:
    0 8px 22px rgba(2, 8, 20, 0.28),
    inset 0 1px 0 rgba(255, 255, 255, 0.18);
  transition: opacity 0.16s ease, transform 0.16s ease, filter 0.16s ease;
}

.beeroom-canvas-board:hover .beeroom-canvas-chat-handle,
.beeroom-canvas-board:focus-within .beeroom-canvas-chat-handle {
  opacity: 1;
  pointer-events: auto;
}

.beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle {
  left: -18px;
  opacity: 0.18;
  pointer-events: auto;
}

.beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle:hover,
.beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle:focus-visible,
.beeroom-canvas-board:hover .beeroom-canvas-chat.collapsed .beeroom-canvas-chat-handle {
  opacity: 1;
}

.beeroom-canvas-chat-handle:hover,
.beeroom-canvas-chat-handle:focus-visible {
  transform: translateY(-50%) scale(1.02);
  filter: drop-shadow(0 0 12px rgba(56, 189, 248, 0.32));
}

.beeroom-canvas-chat-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
  padding-bottom: 4px;
  border-bottom: 1px solid rgba(56, 189, 248, 0.08);
}

.beeroom-canvas-chat-head-actions {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}

.beeroom-canvas-icon-btn {
  width: 28px;
  height: 28px;
  border-radius: 10px;
  border: 1px solid rgba(56, 189, 248, 0.18);
  background: rgba(15, 23, 42, 0.7);
  color: #bae6fd;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
}

.beeroom-canvas-icon-btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.beeroom-canvas-chat-title {
  color: #f8fafc;
  font-size: 16px;
  font-weight: 700;
  letter-spacing: 0.02em;
}

.beeroom-canvas-chat-subtitle,
.beeroom-canvas-chat-time,
.beeroom-canvas-chat-extra,
.beeroom-canvas-chat-overview-label {
  color: rgba(191, 219, 254, 0.72);
  font-size: 11px;
}

.beeroom-canvas-chat-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 24px;
  padding: 0 8px;
  border-radius: 999px;
  background: rgba(14, 116, 144, 0.14);
  border: 1px solid rgba(56, 189, 248, 0.16);
  color: #67e8f9;
  font-size: 11px;
}

.beeroom-canvas-chat-stream {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
  padding-right: 2px;
}

.beeroom-canvas-chat-message {
  display: flex;
  align-items: flex-start;
  gap: 10px;
}

.beeroom-canvas-chat-avatar {
  width: 34px;
  height: 34px;
  border: 1px solid rgba(56, 189, 248, 0.18);
  border-radius: 12px;
  background: rgba(14, 116, 144, 0.16);
  color: #bae6fd;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 12px;
  font-weight: 700;
  flex-shrink: 0;
  cursor: pointer;
}

.beeroom-canvas-chat-avatar.is-system {
  cursor: default;
  background: rgba(30, 41, 59, 0.78);
  color: #94a3b8;
}

.beeroom-canvas-chat-main {
  display: grid;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.beeroom-canvas-chat-meta-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.beeroom-canvas-chat-sender {
  padding: 0;
  border: none;
  background: transparent;
  color: #f8fafc;
  font-size: 12px;
  font-weight: 700;
  cursor: pointer;
}

.beeroom-canvas-chat-sender.is-system {
  color: #cbd5e1;
  cursor: default;
}

.beeroom-canvas-chat-bubble {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 10px 12px;
  border-radius: 14px;
  border: 1px solid rgba(148, 163, 184, 0.12);
  background: linear-gradient(180deg, rgba(15, 23, 42, 0.8), rgba(8, 15, 32, 0.74));
  color: #e2e8f0;
  font-size: 12.5px;
  line-height: 1.65;
  box-shadow: inset 0 1px 0 rgba(186, 230, 253, 0.03);
}

.beeroom-canvas-chat-message.is-mother .beeroom-canvas-chat-bubble {
  border-color: rgba(56, 189, 248, 0.18);
  background: rgba(8, 47, 73, 0.36);
}

.beeroom-canvas-chat-message.is-worker .beeroom-canvas-chat-bubble {
  border-color: rgba(45, 212, 191, 0.18);
  background: rgba(6, 78, 59, 0.24);
}

.beeroom-canvas-chat-message.is-system .beeroom-canvas-chat-bubble {
  border-style: dashed;
  background: rgba(30, 41, 59, 0.52);
}

.beeroom-canvas-chat-message.is-user .beeroom-canvas-chat-bubble {
  border-color: rgba(125, 211, 252, 0.28);
  background: rgba(8, 47, 73, 0.52);
}

.beeroom-canvas-chat-mention {
  color: #67e8f9;
  font-weight: 700;
}

.beeroom-canvas-chat-avatar.is-user {
  cursor: default;
  background: rgba(8, 47, 73, 0.82);
  color: #e0f2fe;
}

.beeroom-canvas-chat-sender.is-user {
  color: #e0f2fe;
}

.beeroom-canvas-chat-composer {
  display: grid;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid rgba(56, 189, 248, 0.12);
  background: linear-gradient(180deg, rgba(8, 15, 32, 0), rgba(8, 15, 32, 0.42));
}

.beeroom-canvas-chat-compose-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.beeroom-canvas-chat-compose-status {
  color: rgba(191, 219, 254, 0.72);
  font-size: 11px;
}

.beeroom-canvas-chat-compose-status.is-error {
  color: #fca5a5;
}

.beeroom-canvas-chat-textarea {
  width: 100%;
  border-radius: 12px;
  border: 1px solid rgba(56, 189, 248, 0.16);
  background: linear-gradient(180deg, rgba(15, 23, 42, 0.9), rgba(8, 15, 32, 0.86));
  color: #f8fafc;
  outline: none;
  box-shadow: inset 0 1px 0 rgba(186, 230, 253, 0.03);
}

.beeroom-canvas-chat-select {
  flex: 1;
  min-width: 0;
}

.beeroom-canvas-chat-select :deep(.el-select__wrapper) {
  min-height: 38px;
  padding: 0 10px;
  border-radius: 12px;
  border: 1px solid rgba(56, 189, 248, 0.16);
  background: linear-gradient(180deg, rgba(15, 23, 42, 0.9), rgba(8, 15, 32, 0.86));
  box-shadow: inset 0 1px 0 rgba(186, 230, 253, 0.03);
}

.beeroom-canvas-chat-select :deep(.el-select__selected-item),
.beeroom-canvas-chat-select :deep(.el-select__placeholder),
.beeroom-canvas-chat-select :deep(.el-select__input) {
  color: #f8fafc;
}

.beeroom-canvas-chat-select :deep(.el-select__caret) {
  color: rgba(191, 219, 254, 0.84);
}

.beeroom-canvas-chat-select :deep(.is-focused .el-select__wrapper),
.beeroom-canvas-chat-select :deep(.el-select__wrapper.is-focused) {
  box-shadow:
    0 0 0 1px rgba(56, 189, 248, 0.2),
    inset 0 1px 0 rgba(186, 230, 253, 0.04);
}

.beeroom-canvas-chat-textarea {
  resize: none;
  min-height: 84px;
  padding: 10px 12px;
  line-height: 1.6;
}

:deep(.beeroom-canvas-chat-select-popper.el-popper) {
  border: 1px solid rgba(56, 189, 248, 0.18);
  background: linear-gradient(180deg, rgba(15, 23, 42, 0.98), rgba(8, 15, 32, 0.98));
  box-shadow: 0 18px 40px rgba(2, 6, 23, 0.42);
}

:deep(.beeroom-canvas-chat-select-popper.el-popper .el-popper__arrow::before) {
  border-color: rgba(56, 189, 248, 0.18);
  background: rgba(11, 18, 32, 0.98);
}

:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item) {
  color: #e2e8f0;
}

:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item.is-hovering),
:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item:hover) {
  background: rgba(8, 47, 73, 0.72);
}

:deep(.beeroom-canvas-chat-select-popper .el-select-dropdown__item.is-selected) {
  color: #67e8f9;
  background: rgba(8, 47, 73, 0.9);
}

.beeroom-canvas-chat-send {
  min-width: 74px;
  min-height: 34px;
  padding: 0 12px;
  border-radius: 12px;
  border: 1px solid rgba(34, 211, 238, 0.26);
  background: linear-gradient(135deg, rgba(8, 145, 178, 0.9), rgba(37, 99, 235, 0.9));
  color: #eff6ff;
  cursor: pointer;
  box-shadow: 0 10px 24px rgba(14, 116, 144, 0.2);
}

.beeroom-canvas-chat-send:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.beeroom-canvas-surface {
  position: relative;
  z-index: 1;
  flex: 1;
  width: 100%;
  height: 100%;
  min-height: 0;
}

.beeroom-canvas-minimap-shell {
  position: absolute;
  left: 14px;
  bottom: 14px;
  z-index: 4;
  display: flex;
  flex-direction: column;
  gap: 6px;
  pointer-events: none;
}

.beeroom-canvas-minimap-label {
  align-self: flex-start;
  padding: 3px 8px;
  border-radius: 999px;
  background: linear-gradient(180deg, rgba(15, 23, 42, 0.94), rgba(8, 15, 32, 0.86));
  border: 1px solid rgba(56, 189, 248, 0.2);
  color: rgba(186, 230, 253, 0.9);
  font-size: 11px;
  letter-spacing: 0.04em;
}

.beeroom-canvas-minimap {
  width: 184px;
  height: 112px;
  overflow: hidden;
  border-radius: 14px;
  border: 1px solid rgba(56, 189, 248, 0.22);
  background: linear-gradient(180deg, rgba(7, 14, 29, 0.92), rgba(2, 8, 20, 0.88));
  box-shadow:
    inset 0 1px 0 rgba(186, 230, 253, 0.03),
    0 10px 26px rgba(2, 6, 23, 0.28);
  pointer-events: auto;
}

.beeroom-canvas-detail {
  position: relative;
  z-index: 1;
  display: flex;
  width: 296px;
  min-width: 296px;
  flex-direction: column;
  gap: 12px;
  padding: 14px;
  border-radius: 16px;
  border: 1px solid rgba(56, 189, 248, 0.16);
  background: rgba(8, 15, 32, 0.84);
  color: #e2e8f0;
  box-shadow: inset 0 0 0 1px rgba(148, 163, 184, 0.05);
  overflow: hidden;
}

.beeroom-canvas-detail-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.beeroom-canvas-detail-head-main {
  display: flex;
  flex: 1;
  min-width: 0;
  flex-direction: column;
  gap: 6px;
}

.beeroom-canvas-detail-title-row,
.beeroom-canvas-detail-status-row,
.beeroom-canvas-task-head,
.beeroom-canvas-section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.beeroom-canvas-detail-title {
  margin: 0;
  font-size: 17px;
  font-weight: 700;
  color: #f8fafc;
}

.beeroom-canvas-role-chip,
.beeroom-canvas-status-chip,
.beeroom-canvas-entry-flag {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 22px;
  padding: 0 9px;
  border-radius: 999px;
  font-size: 11px;
  white-space: nowrap;
}

.beeroom-canvas-role-chip,
.beeroom-canvas-entry-flag {
  border: 1px solid rgba(56, 189, 248, 0.2);
  background: rgba(14, 116, 144, 0.14);
  color: #67e8f9;
}

.beeroom-canvas-status-chip.tone-muted {
  background: rgba(148, 163, 184, 0.14);
  color: #94a3b8;
}

.beeroom-canvas-status-chip.tone-running {
  background: rgba(59, 130, 246, 0.16);
  color: #7dd3fc;
}

.beeroom-canvas-status-chip.tone-success {
  background: rgba(34, 197, 94, 0.16);
  color: #86efac;
}

.beeroom-canvas-status-chip.tone-danger {
  background: rgba(239, 68, 68, 0.16);
  color: #fca5a5;
}

.beeroom-canvas-status-chip.tone-warn {
  background: rgba(245, 158, 11, 0.16);
  color: #fcd34d;
}

.beeroom-canvas-open-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 34px;
  padding: 0 10px;
  border-radius: 12px;
  border: 1px solid rgba(56, 189, 248, 0.26);
  background: rgba(14, 116, 144, 0.16);
  color: #bae6fd;
  cursor: pointer;
  font-size: 12px;
}

.beeroom-canvas-detail-desc {
  margin: 0;
  line-height: 1.5;
  font-size: 13px;
  color: rgba(226, 232, 240, 0.78);
}

.beeroom-canvas-stat-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 8px;
}

.beeroom-canvas-stat-card,
.beeroom-canvas-task-item {
  border: 1px solid rgba(148, 163, 184, 0.14);
  border-radius: 14px;
  background: rgba(15, 23, 42, 0.72);
}

.beeroom-canvas-stat-card {
  padding: 10px;
}

.beeroom-canvas-stat-label {
  color: rgba(148, 163, 184, 0.9);
  font-size: 11px;
}

.beeroom-canvas-stat-value {
  margin-top: 6px;
  font-size: 15px;
  font-weight: 700;
  color: #f8fafc;
}

.beeroom-canvas-subsection,
.beeroom-canvas-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.beeroom-canvas-section {
  flex: 1;
  min-height: 0;
}

.beeroom-canvas-section--tasks {
  min-height: 0;
}

.beeroom-canvas-section-head h4 {
  margin: 0;
  font-size: 14px;
  font-weight: 700;
}

.beeroom-canvas-section-head span {
  color: rgba(148, 163, 184, 0.9);
  font-size: 11px;
}

.beeroom-canvas-kv-list,
.beeroom-canvas-link-list {
  display: grid;
  gap: 8px;
}

.beeroom-canvas-kv-item,
.beeroom-canvas-link-item {
  border: 1px solid rgba(148, 163, 184, 0.14);
  border-radius: 12px;
  background: rgba(15, 23, 42, 0.72);
  padding: 10px;
}

.beeroom-canvas-kv-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.beeroom-canvas-kv-label,
.beeroom-canvas-link-subtitle,
.beeroom-canvas-link-meta,
.beeroom-canvas-tool-summary-text {
  color: rgba(191, 219, 254, 0.72);
  font-size: 11px;
}

.beeroom-canvas-kv-value,
.beeroom-canvas-link-title {
  color: #f8fafc;
  font-size: 13px;
  font-weight: 600;
}

.beeroom-canvas-link-item {
  display: grid;
  gap: 4px;
}

.beeroom-canvas-tool-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.beeroom-canvas-tool-tag {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 22px;
  padding: 0 8px;
  border-radius: 999px;
  border: 1px solid rgba(56, 189, 248, 0.18);
  background: rgba(14, 116, 144, 0.14);
  color: #bae6fd;
  font-size: 11px;
}

.beeroom-canvas-task-list {
  display: grid;
  gap: 8px;
}

.beeroom-canvas-task-item {
  padding: 10px;
}

.beeroom-canvas-task-id {
  color: #f8fafc;
  font-weight: 600;
}

.beeroom-canvas-task-body,
.beeroom-canvas-section-empty,
.beeroom-canvas-detail-empty {
  color: rgba(226, 232, 240, 0.74);
  line-height: 1.6;
}

.beeroom-canvas-detail-empty,
.beeroom-canvas-empty {
  position: relative;
  z-index: 1;
  display: flex;
  flex: 1;
  min-height: 0;
  align-items: center;
  justify-content: center;
  gap: 10px;
  text-align: center;
}

.beeroom-canvas-empty i,
.beeroom-canvas-detail-empty i {
  color: #38bdf8;
}

.beeroom-canvas-minimap :deep(canvas),
.beeroom-canvas-minimap :deep(svg),
.beeroom-canvas-surface :deep(canvas),
.beeroom-canvas-surface :deep(svg) {
  display: block;
  width: 100% !important;
  height: 100% !important;
}

.beeroom-canvas-minimap :deep(.minimap) {
  width: 100% !important;
  height: 100% !important;
}

@media (max-width: 1240px) {
  .beeroom-canvas-board {
    --beeroom-chat-width: 304px;
    grid-template-columns: minmax(0, 1fr) var(--beeroom-chat-width);
  }

  .beeroom-canvas-board.chat-collapsed {
    grid-template-columns: minmax(0, 1fr) 0px;
  }
}
</style>
