<template>
  <div class="swarm-panel">
    <div class="swarm-panel-header">
      <div>
        <div class="swarm-panel-title">{{ t('beehive.swarm.title') }}</div>
      </div>
      <button class="swarm-panel-refresh" type="button" :disabled="loading" @click="loadRuns">
        <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
      </button>
    </div>
    <div v-if="loading" class="swarm-panel-empty">{{ t('common.loading') }}</div>
    <div v-else-if="!runs.length" class="swarm-panel-empty">{{ t('beehive.swarm.empty') }}</div>
    <div v-else class="swarm-panel-list">
      <button
        v-for="run in runs"
        :key="run.team_run_id"
        class="swarm-panel-item"
        :class="{ active: run.team_run_id === activeRunId }"
        type="button"
        @click="loadRunDetail(run.team_run_id)"
      >
        <div class="swarm-panel-item-top">
          <span>{{ run.status }}</span>
          <span>{{ formatTime(run.updated_time) }}</span>
        </div>
        <div class="swarm-panel-item-bottom">
          <span>{{ t('beehive.swarm.tasks', { total: run.task_total }) }}</span>
          <span>{{ run.strategy || '-' }}</span>
        </div>
      </button>
    </div>
    <div v-if="tasks.length" class="swarm-panel-tasks">
      <div class="swarm-panel-tasks-title">{{ t('beehive.swarm.taskLane') }}</div>
      <div class="swarm-task-item" v-for="task in tasks" :key="task.task_id">
        <span>{{ task.agent_id }}</span>
        <span>{{ task.status }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';

import { getTeamRun, listSessionTeamRuns } from '@/api/swarm';
import { useI18n } from '@/i18n';

const props = defineProps({
  sessionId: {
    type: String,
    default: ''
  },
});

const { t } = useI18n();

const loading = ref(false);
const runs = ref([]);
const tasks = ref([]);
const activeRunId = ref('');

const formatTime = (value) => {
  if (!value) return '-';
  const ts = Number(value) * 1000;
  if (!Number.isFinite(ts)) return '-';
  const date = new Date(ts);
  if (Number.isNaN(date.getTime())) return '-';
  const pad = (part) => String(part).padStart(2, '0');
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
};

const loadRunDetail = async (teamRunId) => {
  const runId = String(teamRunId || '').trim();
  if (!runId) return;
  activeRunId.value = runId;
  try {
    const { data } = await getTeamRun(runId);
    tasks.value = data?.data?.tasks || [];
  } catch (error) {
    tasks.value = [];
  }
};

const loadRuns = async () => {
  const sessionId = String(props.sessionId || '').trim();
  if (!sessionId) {
    runs.value = [];
    tasks.value = [];
    activeRunId.value = '';
    return;
  }
  loading.value = true;
  try {
    const params = { limit: 20 };
    const { data } = await listSessionTeamRuns(sessionId, params);
    runs.value = data?.data?.items || [];
    if (!runs.value.find((item) => item.team_run_id === activeRunId.value)) {
      activeRunId.value = runs.value[0]?.team_run_id || '';
      if (activeRunId.value) {
        await loadRunDetail(activeRunId.value);
      } else {
        tasks.value = [];
      }
    }
  } catch (error) {
    runs.value = [];
    tasks.value = [];
    activeRunId.value = '';
  } finally {
    loading.value = false;
  }
};

watch(
  () => props.sessionId,
  () => {
    loadRuns();
  },
  { immediate: true }
);
</script>
