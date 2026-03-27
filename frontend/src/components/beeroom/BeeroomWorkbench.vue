<template>
  <section class="beeroom-workbench">
    <div v-if="loading && !group" class="beeroom-state beeroom-state--loading">{{ t('common.loading') }}</div>
    <div v-else-if="!group" class="beeroom-state">
      <i class="fa-solid fa-hexagon-nodes" aria-hidden="true"></i>
      <span>{{ t('beeroom.empty.selectGroup') }}</span>
    </div>
    <template v-else>
      <header class="beeroom-workbench-toolbar">
        <div class="beeroom-workbench-main">
          <div class="beeroom-workbench-topline">
            <span class="beeroom-workbench-badge">{{ t('messenger.section.swarms') }}</span>
            <span class="beeroom-status-chip" :class="resolveGroupTone(group.status)">
              {{ resolveGroupStatus(group.status) }}
            </span>
            <span class="beeroom-workbench-group-id">#{{ group.group_id }}</span>
          </div>
          <h2 class="beeroom-workbench-title">{{ group.name || group.group_id }}</h2>
          <p class="beeroom-workbench-description">
            {{ group.description || t('beeroom.empty.description') }}
          </p>
          <div class="beeroom-workbench-meta">
            <span>{{ t('beeroom.summary.motherAgent') }}: {{ motherAgentLabel }}</span>
            <span>{{ t('beeroom.summary.latestMission') }}: {{ latestMissionLabel }}</span>
            <span v-if="error" class="beeroom-workbench-error">{{ error }}</span>
          </div>
        </div>

        <div class="beeroom-workbench-actions">
          <button class="beeroom-action-btn" type="button" @click="emit('refresh')">
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

      <div class="beeroom-workbench-summary">
        <article v-for="card in summaryCards" :key="card.key" class="beeroom-workbench-summary-card">
          <div class="beeroom-workbench-summary-label">{{ card.label }}</div>
          <div class="beeroom-workbench-summary-value">{{ card.value }}</div>
          <div class="beeroom-workbench-summary-hint">{{ card.hint }}</div>
        </article>
      </div>

      <div v-if="orderedMissions.length" class="beeroom-workbench-missions">
        <button
          v-for="mission in orderedMissions"
          :key="mission.mission_id || mission.team_run_id"
          class="beeroom-workbench-mission-chip"
          :class="{ active: selectedMissionId === (mission.mission_id || mission.team_run_id) }"
          type="button"
          :aria-pressed="selectedMissionId === (mission.mission_id || mission.team_run_id)"
          @click="selectedMissionId = mission.mission_id || mission.team_run_id"
        >
          <span class="beeroom-workbench-mission-chip-title">{{ resolveMissionTitle(mission) }}</span>
          <span class="beeroom-workbench-mission-chip-meta">
            #{{ shortMissionId(mission.mission_id || mission.team_run_id) }}
            ·
            {{ resolveMissionStatus(mission.completion_status || mission.status) }}
            ·
            {{ formatDateTime(mission.updated_time || mission.started_time) }}
          </span>
        </button>
      </div>

      <div class="beeroom-workbench-stage">
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

      <el-dialog
        v-model="moveDialogVisible"
        width="420px"
        :show-close="false"
        append-to-body
        class="messenger-modal messenger-modal--beeroom"
      >
        <template #header>
          <div class="messenger-modal-header">
            <div>
              <div class="messenger-modal-title">{{ t('beeroom.dialog.moveTitle') }}</div>
              <div class="messenger-modal-subtitle">{{ t('beeroom.dialog.moveSubtitle') }}</div>
            </div>
            <button class="messenger-dialog-close" type="button" @click="moveDialogVisible = false">&times;</button>
          </div>
        </template>

        <div class="beeroom-move-copy">{{ t('beeroom.dialog.moveCount', { count: moveAgentIds.length }) }}</div>
        <el-checkbox-group v-model="moveAgentIds" class="messenger-dialog-grid">
          <el-checkbox v-for="agent in availableAgents" :key="agent.id" :value="agent.id" class="messenger-dialog-check">
            <div class="messenger-dialog-check-main">
              <span>{{ resolveDisplayAgentName(agent.id, agent.name) }}</span>
              <span class="messenger-dialog-check-meta">{{ agent.id }}</span>
            </div>
          </el-checkbox>
        </el-checkbox-group>

        <template #footer>
          <div class="messenger-modal-footer">
            <el-button @click="moveDialogVisible = false">{{ t('common.cancel') }}</el-button>
            <el-button type="primary" :disabled="!moveAgentIds.length" @click="submitMoveAgents">
              {{ t('beeroom.action.moveAgents') }}
            </el-button>
          </div>
        </template>
      </el-dialog>
    </template>
  </section>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import BeeroomMissionCanvas from '@/components/beeroom/BeeroomMissionCanvas.vue';
import { useI18n } from '@/i18n';
import {
  type BeeroomGroup,
  type BeeroomMember,
  type BeeroomMission
} from '@/stores/beeroom';

const selectedMissionCacheByGroup = new Map<string, string>();

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
}>();

const emit = defineEmits<{
  (event: 'refresh'): void;
  (event: 'move-agents', value: string[]): void;
  (event: 'open-agent', agentId: string): void;
}>();

const { t } = useI18n();

const selectedMissionId = ref('');
const moveDialogVisible = ref(false);
const moveAgentIds = ref<string[]>([]);

const resolveGroupScopeKey = (value: unknown) => String(value || '').trim();

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

const resolveMissionMoment = (mission: BeeroomMission) =>
  Number(mission.updated_time || mission.finished_time || mission.started_time || 0);

const resolveMissionTitle = (mission: BeeroomMission) =>
  String(mission.summary || mission.strategy || '').trim() ||
  `${t('beeroom.summary.latestMission')} #${shortMissionId(mission.mission_id || mission.team_run_id)}`;

const orderedMissions = computed(() =>
  [...props.missions].sort((left, right) => resolveMissionMoment(right) - resolveMissionMoment(left))
);

const latestMissionLabel = computed(() => {
  const latestMission = props.group?.latest_mission || orderedMissions.value[0] || null;
  if (!latestMission) return t('beeroom.missions.empty');
  return resolveMissionStatus(latestMission.completion_status || latestMission.status);
});

const motherAgentLabel = computed(() =>
  resolveDisplayAgentName(props.group?.mother_agent_id, props.group?.mother_agent_name)
);

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
    value: props.group?.running_mission_total ?? orderedMissions.value.length,
    hint: t('beeroom.summary.runningTeamsHint')
  }
]);

const selectedMission = computed(() => {
  const selectedId = String(selectedMissionId.value || '').trim();
  if (!selectedId) return orderedMissions.value[0] || null;
  return (
    orderedMissions.value.find((item) => String(item.mission_id || item.team_run_id || '').trim() === selectedId) ||
    orderedMissions.value[0] ||
    null
  );
});

const submitMoveAgents = () => {
  emit('move-agents', [...moveAgentIds.value]);
  moveDialogVisible.value = false;
  moveAgentIds.value = [];
};

watch(
  () => [props.group?.group_id, orderedMissions.value.map((item) => item.mission_id || item.team_run_id).join(',')],
  () => {
    const groupId = resolveGroupScopeKey(props.group?.group_id);
    const missionIds = orderedMissions.value
      .map((item) => String(item.mission_id || item.team_run_id || '').trim())
      .filter(Boolean);
    const cachedMissionId = groupId ? String(selectedMissionCacheByGroup.get(groupId) || '').trim() : '';
    const currentSelected = String(selectedMissionId.value || '').trim();
    const preferredMissionId = currentSelected || cachedMissionId;
    if (!missionIds.length) {
      selectedMissionId.value = preferredMissionId;
      moveAgentIds.value = [];
      moveDialogVisible.value = false;
      return;
    }
    selectedMissionId.value = missionIds.includes(preferredMissionId) ? preferredMissionId : (missionIds[0] || '');
    if (groupId && selectedMissionId.value) {
      selectedMissionCacheByGroup.set(groupId, selectedMissionId.value);
    }
    moveAgentIds.value = [];
    moveDialogVisible.value = false;
  },
  { immediate: true }
);

watch(selectedMissionId, (value) => {
  const groupId = resolveGroupScopeKey(props.group?.group_id);
  const missionId = String(value || '').trim();
  if (!groupId || !missionId) return;
  selectedMissionCacheByGroup.set(groupId, missionId);
});
</script>

<style scoped>
.beeroom-workbench {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 100%;
}

.beeroom-workbench-stage {
  position: relative;
  display: flex;
  flex: 1;
  width: 100%;
  min-height: 0;
}

.beeroom-workbench-canvas {
  display: flex;
  flex: 1;
  width: 100%;
  min-height: 0;
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

.beeroom-workbench-toolbar,
.beeroom-workbench-summary-card {
  border: 1px solid var(--hula-border);
  background: var(--hula-center-bg);
  border-radius: 18px;
}

.beeroom-workbench-toolbar {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  padding: 18px;
}

.beeroom-workbench-main {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
}

.beeroom-workbench-topline,
.beeroom-workbench-meta,
.beeroom-workbench-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px 12px;
  align-items: center;
}

.beeroom-workbench-badge,
.beeroom-workbench-group-id {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  background: rgba(148, 163, 184, 0.14);
  color: var(--hula-muted);
  font-size: 12px;
}

.beeroom-workbench-badge {
  background: var(--hula-accent-soft);
  color: var(--hula-accent);
}

.beeroom-workbench-title {
  margin: 0;
  font-size: 24px;
  line-height: 1.2;
}

.beeroom-workbench-description,
.beeroom-workbench-meta,
.beeroom-workbench-summary-hint,
.beeroom-workbench-mission-chip-meta {
  color: var(--hula-muted);
}

.beeroom-workbench-error {
  color: var(--hula-danger);
}

.beeroom-workbench-actions {
  justify-content: flex-end;
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

.beeroom-workbench-summary {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}

.beeroom-workbench-summary-card {
  padding: 16px;
}

.beeroom-workbench-summary-label {
  color: var(--hula-muted);
  font-size: 12px;
}

.beeroom-workbench-summary-value {
  margin-top: 10px;
  font-size: 28px;
  font-weight: 700;
}

.beeroom-workbench-missions {
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
  max-width: 280px;
  color: #f8fafc;
  font-size: 12px;
  font-weight: 700;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.beeroom-workbench-mission-chip-meta {
  font-size: 11px;
  white-space: nowrap;
}

.beeroom-status-chip {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 24px;
  padding: 0 10px;
  border-radius: 999px;
  font-size: 12px;
}

.beeroom-status-chip.tone-default,
.beeroom-status-chip.tone-muted {
  background: rgba(148, 163, 184, 0.14);
  color: #64748b;
}

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

.beeroom-move-copy {
  margin-bottom: 12px;
  color: var(--hula-muted);
}

@media (max-width: 1280px) {
  .beeroom-workbench-summary {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 900px) {
  .beeroom-workbench-toolbar {
    flex-direction: column;
  }

  .beeroom-workbench-actions {
    width: 100%;
    justify-content: flex-start;
  }

  .beeroom-workbench-summary {
    grid-template-columns: 1fr;
  }

  .beeroom-workbench-mission-chip {
    min-width: 220px;
  }
}
</style>
