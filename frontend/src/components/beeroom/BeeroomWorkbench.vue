<template>
  <section class="beeroom-workbench" :class="{ 'beeroom-workbench--canvas': isCanvasView }">
    <div v-if="loading" class="beeroom-state beeroom-state--loading">{{ t('common.loading') }}</div>
    <div v-else-if="!group" class="beeroom-state">
      <i class="fa-solid fa-hexagon-nodes" aria-hidden="true"></i>
      <span>{{ t('beeroom.empty.selectGroup') }}</span>
    </div>
    <template v-else>
      <header v-if="isTextView" class="beeroom-hero">
        <div class="beeroom-hero-main">
          <div class="beeroom-eyebrow">
            <span class="beeroom-eyebrow-badge">{{ t('messenger.section.swarms') }}</span>
            <span class="beeroom-eyebrow-id">{{ group.group_id }}</span>
          </div>
          <div class="beeroom-title-row">
            <h2 class="beeroom-title">{{ group.name || group.group_id }}</h2>
            <span class="beeroom-status-chip" :class="resolveGroupTone(group.status)">
              {{ resolveGroupStatus(group.status) }}
            </span>
          </div>
          <p class="beeroom-description">
            {{ group.description || t('beeroom.empty.description') }}
          </p>
          <div class="beeroom-meta-row">
            <span>{{ t('beeroom.summary.motherAgent') }}: {{ group.mother_agent_name || group.mother_agent_id || t('common.none') }}</span>
            <span>{{ t('beeroom.summary.latestMission') }}: {{ latestMissionLabel }}</span>
            <span v-if="error" class="beeroom-meta-error">{{ error }}</span>
          </div>
        </div>

        <div class="beeroom-hero-actions">
          <button class="beeroom-action-btn" type="button" @click="$emit('refresh')">
            <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
            <span>{{ refreshing ? t('common.loading') : t('common.refresh') }}</span>
          </button>
          <button
            class="beeroom-action-btn beeroom-action-btn--primary"
            type="button"
            :disabled="!availableAgents.length"
            @click="moveDialogVisible = true"
          >
            <i class="fa-solid fa-share-nodes" aria-hidden="true"></i>
            <span>{{ t('beeroom.action.moveAgents') }}</span>
          </button>
        </div>
      </header>

      <div v-if="isTextView" class="beeroom-summary-grid">
        <article v-for="card in summaryCards" :key="card.key" class="beeroom-summary-card">
          <div class="beeroom-summary-label">{{ card.label }}</div>
          <div class="beeroom-summary-value">{{ card.value }}</div>
          <div class="beeroom-summary-hint">{{ card.hint }}</div>
        </article>
      </div>

      <div v-if="isTextView" class="beeroom-content-grid">
        <section v-if="isTextView" class="beeroom-panel beeroom-panel--members">
          <div class="beeroom-panel-head">
            <div>
              <div class="beeroom-panel-title">{{ t('beeroom.members.title') }}</div>
              <div class="beeroom-panel-subtitle">{{ t('beeroom.members.subtitle') }}</div>
            </div>
            <span class="beeroom-panel-tag">{{ agents.length }}</span>
          </div>

          <div v-if="!agents.length" class="beeroom-panel-empty">{{ t('beeroom.members.empty') }}</div>
          <div v-else class="beeroom-member-list">
            <article
              v-for="member in agents"
              :key="member.agent_id"
              class="beeroom-member-card"
              @dblclick="$emit('open-agent', member.agent_id)"
            >
              <div class="beeroom-member-head">
                <div class="beeroom-member-avatar">
                  {{ avatarLabel(resolveDisplayAgentName(member.agent_id, member.name)) }}
                </div>
                <div class="beeroom-member-main">
                  <div class="beeroom-member-name-row">
                    <span class="beeroom-member-name">
                      {{ resolveDisplayAgentName(member.agent_id, member.name) }}
                    </span>
                    <span class="beeroom-member-state" :class="member.idle === false ? 'active' : 'idle'">
                      {{ member.idle === false ? t('beeroom.members.active') : t('beeroom.members.idle') }}
                    </span>
                  </div>
                  <div class="beeroom-member-desc">
                    {{ member.description || t('beeroom.members.noDescription') }}
                  </div>
                </div>
              </div>
              <div class="beeroom-member-foot">
                <span>{{ t('beeroom.members.sessions', { count: member.active_session_total || 0 }) }}</span>
                <span>{{ member.approval_mode || '-' }}</span>
                <button class="beeroom-inline-link" type="button" @click="$emit('open-agent', member.agent_id)">
                  {{ t('beeroom.canvas.openChat') }}
                </button>
              </div>
            </article>
          </div>
        </section>

        <section class="beeroom-panel beeroom-panel--missions">
          <div class="beeroom-panel-head">
            <div>
              <div class="beeroom-panel-title">{{ t('beeroom.missions.title') }}</div>
              <div class="beeroom-panel-subtitle">{{ t('beeroom.missions.subtitle') }}</div>
            </div>
            <span class="beeroom-panel-tag">{{ missions.length }}</span>
          </div>

          <div v-if="!missions.length" class="beeroom-panel-empty">{{ t('beeroom.missions.empty') }}</div>
          <div v-else class="beeroom-mission-buckets">
            <section v-for="bucket in missionBuckets" :key="bucket.key" class="beeroom-mission-bucket">
              <div class="beeroom-mission-bucket-head">
                <span>{{ bucket.label }}</span>
                <span class="beeroom-mission-bucket-count">{{ bucket.items.length }}</span>
              </div>
              <button
                v-for="mission in bucket.items"
                :key="mission.mission_id || mission.team_run_id"
                class="beeroom-mission-card"
                :class="{ active: selectedMissionId === (mission.mission_id || mission.team_run_id) }"
                type="button"
                @click="selectedMissionId = mission.mission_id || mission.team_run_id"
              >
                <div class="beeroom-mission-card-head">
                  <span class="beeroom-mission-card-id">#{{ shortMissionId(mission.mission_id || mission.team_run_id) }}</span>
                  <span class="beeroom-status-chip" :class="resolveMissionTone(mission.completion_status || mission.status)">
                    {{ resolveMissionStatus(mission.completion_status || mission.status) }}
                  </span>
                </div>
                <div class="beeroom-mission-card-summary">{{ mission.summary || mission.strategy || t('beeroom.missions.noSummary') }}</div>
                <div class="beeroom-mission-card-foot">
                  <span>{{ t('beeroom.missions.taskCount', { count: mission.task_total || 0 }) }}</span>
                  <span>{{ formatDateTime(mission.updated_time || mission.started_time) }}</span>
                </div>
              </button>
            </section>
          </div>
        </section>
      </div>

      <div v-if="isCanvasView" class="beeroom-workbench-stage">
        <BeeroomMissionCanvas
          class="beeroom-workbench-canvas"
          :group="group"
          :mission="selectedMission"
          :agents="agents"
          :refreshing="refreshing"
          @refresh="emit('refresh')"
          @open-agent="emit('open-agent', $event)"
        />
      </div>

      <section v-if="isTextView" class="beeroom-panel beeroom-panel--detail">
        <div class="beeroom-panel-head">
          <div>
            <div class="beeroom-panel-title">{{ t('beeroom.missionDetail.title') }}</div>
            <div class="beeroom-panel-subtitle">{{ t('beeroom.missionDetail.subtitle') }}</div>
          </div>
          <span v-if="selectedMission" class="beeroom-panel-tag">
            {{ t('beeroom.missions.taskCount', { count: selectedMission.task_total || 0 }) }}
          </span>
        </div>

        <div v-if="!selectedMission" class="beeroom-panel-empty">{{ t('beeroom.missionDetail.empty') }}</div>
        <template v-else>
          <div class="beeroom-detail-head">
            <div>
              <div class="beeroom-detail-title">
                {{ selectedMission.summary || selectedMission.strategy || selectedMission.mission_id }}
              </div>
              <div class="beeroom-detail-meta">
                <span>{{ t('beeroom.summary.motherAgent') }}: {{ selectedMission.mother_agent_id || group.mother_agent_id || t('common.none') }}</span>
                <span>{{ t('beeroom.missionDetail.entryAgent') }}: {{ selectedMission.entry_agent_id || t('common.none') }}</span>
                <span>{{ t('beeroom.missionDetail.parentSession') }}: {{ selectedMission.parent_session_id || '-' }}</span>
              </div>
            </div>
            <div class="beeroom-detail-stats">
              <span>{{ t('beeroom.missionDetail.success', { count: selectedMission.task_success || 0 }) }}</span>
              <span>{{ t('beeroom.missionDetail.failed', { count: selectedMission.task_failed || 0 }) }}</span>
              <span>{{ t('beeroom.missionDetail.tokens', { count: selectedMission.context_tokens_total || 0 }) }}</span>
            </div>
          </div>

          <div class="beeroom-task-grid">
            <article
              v-for="task in selectedMission.tasks || []"
              :key="task.task_id"
              class="beeroom-task-card"
              @dblclick="$emit('open-agent', task.agent_id)"
            >
              <div class="beeroom-task-head">
                <span class="beeroom-task-name">{{ resolveAgentName(task.agent_id) }}</span>
                <span class="beeroom-status-chip" :class="resolveMissionTone(task.status)">
                  {{ resolveMissionStatus(task.status) }}
                </span>
              </div>
              <div class="beeroom-task-meta">
                <span>{{ t('beeroom.task.priority') }} {{ task.priority ?? 0 }}</span>
                <span>{{ t('beeroom.task.runId') }} {{ shortMissionId(task.session_run_id || '-') }}</span>
                <button class="beeroom-inline-link" type="button" @click="$emit('open-agent', task.agent_id)">
                  {{ t('beeroom.canvas.openChat') }}
                </button>
              </div>
              <div class="beeroom-task-body">
                {{ task.result_summary || task.error || t('beeroom.task.pending') }}
              </div>
            </article>
          </div>
        </template>
      </section>
    </template>

    <el-dialog
      v-model="moveDialogVisible"
      class="messenger-dialog"
      width="520px"
      top="12vh"
      append-to-body
    >
      <template #header>
        <div class="messenger-dialog-header">
          <div class="messenger-dialog-title">{{ t('beeroom.dialog.moveTitle') }}</div>
          <button class="messenger-dialog-close" type="button" @click="moveDialogVisible = false">&times;</button>
        </div>
      </template>
      <div class="messenger-dialog-body">
        <div class="beeroom-move-copy">{{ t('beeroom.dialog.moveCount', { count: moveAgentIds.length }) }}</div>
        <el-select
          v-model="moveAgentIds"
          multiple
          filterable
          class="messenger-form-full"
          :placeholder="t('beeroom.dialog.moveTargetPlaceholder')"
        >
          <el-option
            v-for="agent in availableAgents"
            :key="agent.id"
            :label="agent.name || agent.id"
            :value="agent.id"
          />
        </el-select>
      </div>
      <template #footer>
        <div class="messenger-dialog-footer">
          <el-button @click="moveDialogVisible = false">{{ t('common.cancel') }}</el-button>
          <el-button type="primary" :disabled="!moveAgentIds.length" @click="submitMoveAgents">
            {{ t('common.confirm') }}
          </el-button>
        </div>
      </template>
    </el-dialog>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import { useI18n } from '@/i18n';
import BeeroomMissionCanvas from '@/components/beeroom/BeeroomMissionCanvas.vue';
import {
  type BeeroomGroup,
  type BeeroomMember,
  type BeeroomMission
} from '@/stores/beeroom';

type AgentOption = {
  id: string;
  name?: string;
};

const props = defineProps<{
  group: BeeroomGroup | null;
  agents: BeeroomMember[];
  missions: BeeroomMission[];
  availableAgents: AgentOption[];
  loading: boolean;
  refreshing: boolean;
  error: string;
  viewMode?: 'text' | 'canvas';
}>();

const emit = defineEmits<{
  (event: 'refresh'): void;
  (event: 'move-agents', value: string[]): void;
  (event: 'open-agent', agentId: string): void;
}>();

const { t } = useI18n();

const isCanvasView = computed(() => true);
const isTextView = computed(() => false);

const selectedMissionId = ref('');
const moveDialogVisible = ref(false);
const moveAgentIds = ref<string[]>([]);

const formatDateTime = (value: unknown) => {
  const numeric = Number(value || 0);
  if (!Number.isFinite(numeric) || numeric <= 0) {
    return '-';
  }
  return new Intl.DateTimeFormat(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit'
  }).format(new Date(numeric * 1000));
};

const shortMissionId = (value: unknown) => {
  const text = String(value || '').trim();
  if (!text) return '-';
  return text.length > 10 ? text.slice(-10) : text;
};

const isDefaultAgentId = (value: unknown) => {
  const normalized = String(value || '').trim().toLowerCase();
  return normalized === '__default__' || normalized === 'default';
};

const resolveDisplayAgentName = (agentId: unknown, fallbackName?: unknown) => {
  if (isDefaultAgentId(agentId)) {
    return t('messenger.defaultAgent');
  }
  return String(fallbackName || agentId || '-').trim() || '-';
};

const avatarLabel = (value: unknown) => String(value || '?').trim().slice(0, 1).toUpperCase() || '?';

const resolveGroupTone = (value: unknown) => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'active') return 'tone-running';
  if (normalized === 'archived') return 'tone-muted';
  return 'tone-default';
};

const resolveGroupStatus = (value: unknown) => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'active') return t('beeroom.status.active');
  if (normalized === 'archived') return t('beeroom.status.archived');
  return normalized || t('beeroom.status.unknown');
};

const resolveMissionTone = (value: unknown) => {
  const normalized = String(value || '').trim().toLowerCase();
  if (normalized === 'completed' || normalized === 'success') return 'tone-success';
  if (normalized === 'failed' || normalized === 'error' || normalized === 'timeout' || normalized === 'cancelled') {
    return 'tone-danger';
  }
  if (normalized === 'awaiting_idle') return 'tone-warn';
  if (normalized === 'running' || normalized === 'queued') return 'tone-running';
  return 'tone-default';
};

const resolveMissionStatus = (value: unknown) => {
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
    cancelled: 'beeroom.status.cancelled'
  };
  return t(keyMap[normalized] || 'beeroom.status.unknown');
};

const latestMissionLabel = computed(() => {
  const latestMission = props.group?.latest_mission;
  if (!latestMission) return t('beeroom.missions.empty');
  return resolveMissionStatus(latestMission.completion_status || latestMission.status);
});

const summaryCards = computed(() => [
  {
    key: 'agents',
    label: t('beeroom.summary.agents'),
    value: props.group?.agent_total ?? props.agents.length,
    hint: t('beeroom.summary.agentsHint')
  },
  {
    key: 'active',
    label: t('beeroom.summary.runningAgents'),
    value: props.group?.active_agent_total ?? props.agents.filter((item) => item.idle === false).length,
    hint: t('beeroom.summary.runningAgentsHint')
  },
  {
    key: 'idle',
    label: t('beeroom.summary.idleAgents'),
    value: props.group?.idle_agent_total ?? props.agents.filter((item) => item.idle !== false).length,
    hint: t('beeroom.summary.idleAgentsHint')
  },
  {
    key: 'missions',
    label: t('beeroom.summary.runningTeams'),
    value: props.group?.running_mission_total ?? 0,
    hint: t('beeroom.summary.runningTeamsHint')
  }
]);

const missionBuckets = computed(() => {
  const buckets = [
    { key: 'running', label: t('beeroom.bucket.running'), items: [] as BeeroomMission[] },
    { key: 'awaiting_idle', label: t('beeroom.bucket.awaitingIdle'), items: [] as BeeroomMission[] },
    { key: 'completed', label: t('beeroom.bucket.completed'), items: [] as BeeroomMission[] },
    { key: 'risk', label: t('beeroom.bucket.risk'), items: [] as BeeroomMission[] }
  ];
  props.missions.forEach((mission) => {
    const status = String(mission.completion_status || mission.status || '').trim().toLowerCase();
    if (status === 'completed' || status === 'success') {
      buckets[2].items.push(mission);
      return;
    }
    if (status === 'awaiting_idle') {
      buckets[1].items.push(mission);
      return;
    }
    if (status === 'failed' || status === 'error' || status === 'timeout' || status === 'cancelled') {
      buckets[3].items.push(mission);
      return;
    }
    buckets[0].items.push(mission);
  });
  return buckets;
});

const selectedMission = computed(() => {
  const selectedId = String(selectedMissionId.value || '').trim();
  if (!selectedId) return props.missions[0] || null;
  return (
    props.missions.find((item) => String(item.mission_id || item.team_run_id || '').trim() === selectedId) ||
    props.missions[0] ||
    null
  );
});

const resolveAgentName = (agentId: unknown) => {
  const normalized = String(agentId || '').trim();
  if (!normalized) return '-';
  const member = props.agents.find((item) => item.agent_id === normalized);
  return resolveDisplayAgentName(normalized, member?.name);
};

const submitMoveAgents = () => {
  emit('move-agents', [...moveAgentIds.value]);
  moveDialogVisible.value = false;
  moveAgentIds.value = [];
};

watch(
  () => [props.group?.group_id, props.missions.map((item) => item.mission_id || item.team_run_id).join(',')],
  () => {
    selectedMissionId.value = String(props.missions[0]?.mission_id || props.missions[0]?.team_run_id || '');
    moveAgentIds.value = [];
    moveDialogVisible.value = false;
  },
  { immediate: true }
);
</script>

<style scoped>
.beeroom-workbench {
  display: flex;
  flex-direction: column;
  gap: 16px;
  min-height: 100%;
}

.beeroom-workbench--canvas {
  flex: 1;
  height: 100%;
  min-height: 0;
  gap: 0;
  overflow: hidden;
}

.beeroom-workbench-canvas {
  display: flex;
  flex: 1;
  width: 100%;
  height: 100%;
  min-height: 0;
}

.beeroom-workbench-stage {
  position: relative;
  display: flex;
  flex: 1;
  width: 100%;
  height: 100%;
  min-height: 0;
}

.beeroom-workbench-missions {
  position: absolute;
  left: 14px;
  right: 14px;
  top: 14px;
  z-index: 4;
  display: flex;
  gap: 8px;
  overflow-x: auto;
  padding: 8px 10px 10px;
  border-radius: 16px;
  border: 1px solid rgba(56, 189, 248, 0.12);
  background: linear-gradient(180deg, rgba(8, 15, 32, 0.74), rgba(8, 15, 32, 0.46));
  box-shadow:
    inset 0 1px 0 rgba(186, 230, 253, 0.03),
    0 12px 30px rgba(2, 6, 23, 0.18);
}

.beeroom-workbench-mission-chip {
  display: inline-flex;
  min-width: 0;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
  padding: 10px 12px;
  border-radius: 14px;
  border: 1px solid rgba(56, 189, 248, 0.16);
  background: linear-gradient(180deg, rgba(15, 23, 42, 0.88), rgba(8, 15, 32, 0.82));
  color: #e2e8f0;
  cursor: pointer;
  box-shadow:
    inset 0 1px 0 rgba(186, 230, 253, 0.03),
    0 10px 22px rgba(2, 6, 23, 0.18);
}

.beeroom-workbench-mission-chip.active {
  border-color: rgba(34, 211, 238, 0.42);
  background: linear-gradient(180deg, rgba(8, 47, 73, 0.96), rgba(10, 30, 62, 0.88));
  box-shadow:
    inset 0 0 0 1px rgba(103, 232, 249, 0.08),
    0 12px 26px rgba(8, 47, 73, 0.24);
}

.beeroom-workbench-mission-chip-title {
  color: #f8fafc;
  font-size: 12px;
  font-weight: 700;
}

.beeroom-workbench-mission-chip-meta {
  color: rgba(191, 219, 254, 0.74);
  font-size: 11px;
  white-space: nowrap;
}

.beeroom-state {
  display: flex;
  min-height: 320px;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border: 1px dashed var(--hula-border);
  border-radius: 18px;
  color: var(--hula-muted);
  background: var(--hula-center-bg);
}

.beeroom-hero,
.beeroom-panel,
.beeroom-summary-card {
  border: 1px solid var(--hula-border);
  background: var(--hula-center-bg);
  border-radius: 18px;
}

.beeroom-hero {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  padding: 18px;
}

.beeroom-hero-main {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.beeroom-eyebrow {
  display: flex;
  align-items: center;
  gap: 10px;
  color: var(--hula-muted);
  font-size: 12px;
}

.beeroom-eyebrow-badge,
.beeroom-panel-tag,
.beeroom-mission-bucket-count {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  background: var(--hula-accent-soft);
  color: var(--hula-accent);
}

.beeroom-title-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}

.beeroom-title {
  margin: 0;
  font-size: 24px;
  line-height: 1.2;
}

.beeroom-description,
.beeroom-panel-subtitle,
.beeroom-summary-hint,
.beeroom-member-desc,
.beeroom-task-body,
.beeroom-mission-card-summary,
.beeroom-detail-meta,
.beeroom-meta-row,
.beeroom-task-meta,
.beeroom-mission-card-foot {
  color: var(--hula-muted);
}

.beeroom-meta-row,
.beeroom-detail-meta,
.beeroom-detail-stats,
.beeroom-task-meta,
.beeroom-mission-card-foot,
.beeroom-member-foot {
  display: flex;
  flex-wrap: wrap;
  gap: 10px 14px;
  font-size: 12px;
}

.beeroom-meta-error {
  color: var(--hula-danger);
}

.beeroom-inline-link {
  padding: 0;
  border: none;
  background: transparent;
  color: var(--hula-accent);
  cursor: pointer;
}

.beeroom-hero-actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.beeroom-action-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  min-height: 40px;
  padding: 0 14px;
  border: 1px solid var(--hula-border);
  background: var(--hula-center-bg);
  color: var(--hula-text);
  border-radius: 12px;
  cursor: pointer;
}

.beeroom-action-btn:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.beeroom-action-btn--primary {
  border-color: var(--hula-accent);
  background: var(--hula-accent-soft);
  color: var(--hula-accent);
}

.beeroom-summary-grid,
.beeroom-content-grid,
.beeroom-task-grid,
.beeroom-member-list {
  display: grid;
  gap: 14px;
}

.beeroom-summary-grid {
  grid-template-columns: repeat(4, minmax(0, 1fr));
}

.beeroom-summary-card {
  padding: 16px;
}

.beeroom-summary-label {
  color: var(--hula-muted);
  font-size: 12px;
}

.beeroom-summary-value {
  margin-top: 10px;
  font-size: 28px;
  font-weight: 700;
}

.beeroom-content-grid {
  grid-template-columns: minmax(320px, 0.9fr) minmax(0, 1.1fr);
}

.beeroom-content-grid--canvas {
  grid-template-columns: 1fr;
}

.beeroom-panel {
  padding: 16px;
}

.beeroom-panel-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}

.beeroom-panel-title,
.beeroom-detail-title {
  font-size: 16px;
  font-weight: 700;
}

.beeroom-panel-empty {
  display: flex;
  min-height: 160px;
  align-items: center;
  justify-content: center;
  color: var(--hula-muted);
}

.beeroom-member-card,
.beeroom-task-card,
.beeroom-mission-card,
.beeroom-mission-bucket {
  border: 1px solid var(--hula-border);
  border-radius: 14px;
  background: var(--hula-main-bg);
}

.beeroom-member-card,
.beeroom-task-card,
.beeroom-mission-card {
  padding: 14px;
}

.beeroom-member-head,
.beeroom-task-head,
.beeroom-mission-card-head,
.beeroom-member-name-row,
.beeroom-detail-head,
.beeroom-mission-bucket-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}

.beeroom-member-head {
  align-items: flex-start;
}

.beeroom-member-avatar {
  display: inline-flex;
  width: 36px;
  height: 36px;
  align-items: center;
  justify-content: center;
  border-radius: 12px;
  background: var(--hula-accent-soft);
  color: var(--hula-accent);
  font-weight: 700;
}

.beeroom-member-main {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.beeroom-member-name,
.beeroom-task-name,
.beeroom-mission-card-id {
  font-weight: 700;
}

.beeroom-member-state,
.beeroom-status-chip {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  font-size: 12px;
}

.beeroom-member-state.idle,
.beeroom-status-chip.tone-default,
.beeroom-status-chip.tone-muted {
  background: rgba(148, 163, 184, 0.14);
  color: #64748b;
}

.beeroom-member-state.active,
.beeroom-status-chip.tone-running {
  background: rgba(59, 130, 246, 0.14);
  color: #2563eb;
}

.beeroom-status-chip.tone-success {
  background: rgba(34, 197, 94, 0.14);
  color: #15803d;
}

.beeroom-status-chip.tone-danger {
  background: rgba(239, 68, 68, 0.14);
  color: #b91c1c;
}

.beeroom-status-chip.tone-warn {
  background: rgba(245, 158, 11, 0.15);
  color: #b45309;
}

.beeroom-mission-buckets {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}

.beeroom-mission-bucket {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  min-height: 220px;
}

.beeroom-mission-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 100%;
  text-align: left;
  cursor: pointer;
}

.beeroom-mission-card.active {
  border-color: var(--hula-accent);
}

.beeroom-task-grid {
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
}

.beeroom-move-copy {
  margin-bottom: 12px;
  color: var(--hula-muted);
}

@media (max-width: 1360px) {
  .beeroom-summary-grid,
  .beeroom-mission-buckets {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }

  .beeroom-content-grid {
    grid-template-columns: 1fr;
  }
}

@media (max-width: 860px) {
  .beeroom-hero {
    flex-direction: column;
  }

  .beeroom-hero-actions,
  .beeroom-summary-grid,
  .beeroom-mission-buckets {
    grid-template-columns: 1fr;
  }

  .beeroom-hero-actions {
    flex-direction: row;
    flex-wrap: wrap;
  }
}
</style>

