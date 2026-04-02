<template>
  <section class="beeroom-workbench">
    <div v-if="loading && !group" class="beeroom-state beeroom-state--loading">{{ t('common.loading') }}</div>
    <div v-else-if="!group" class="beeroom-state">
      <i class="fa-solid fa-hexagon-nodes" aria-hidden="true"></i>
      <span>{{ t('beeroom.empty.selectGroup') }}</span>
    </div>
    <template v-else>
      <div class="beeroom-workbench-theater">
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

const resolveMissionMoment = (mission: BeeroomMission) =>
  Number(mission.updated_time || mission.finished_time || mission.started_time || 0);

const orderedMissions = computed(() =>
  [...props.missions].sort((left, right) => resolveMissionMoment(right) - resolveMissionMoment(left))
);

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
      selectedMissionId.value = '';
      if (groupId) {
        selectedMissionCacheByGroup.delete(groupId);
      }
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
  if (!groupId) return;
  if (!missionId) {
    selectedMissionCacheByGroup.delete(groupId);
    return;
  }
  selectedMissionCacheByGroup.set(groupId, missionId);
});
</script>

<style scoped>
.beeroom-workbench {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
  height: 100%;
  color: #e2e8f0;
}

.beeroom-state {
  display: flex;
  min-height: 360px;
  align-items: center;
  justify-content: center;
  gap: 10px;
  border: 1px dashed rgba(148, 163, 184, 0.24);
  border-radius: 24px;
  color: rgba(191, 219, 254, 0.78);
  background:
    radial-gradient(circle at top left, rgba(59, 130, 246, 0.1), transparent 32%),
    linear-gradient(180deg, rgba(7, 10, 18, 0.96), rgba(8, 12, 21, 0.94));
}

.beeroom-workbench-theater {
  display: flex;
  flex: 1;
  min-height: 0;
  flex-direction: column;
  gap: 0;
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

.beeroom-move-copy {
  margin-bottom: 12px;
  color: var(--hula-muted);
}
</style>
