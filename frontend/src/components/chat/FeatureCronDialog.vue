<template>
  <el-dialog
    v-model="visible"
    class="feature-window-dialog feature-window-dialog--cron"
    width="1080px"
    top="8vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="feature-window-header">
        <div class="feature-window-title">{{ t('chat.features.cron') }}</div>
        <button class="feature-window-close" type="button" @click="visible = false">×</button>
      </div>
    </template>
    <div class="feature-window-body">
      <div class="feature-window-toolbar">
        <div class="feature-window-hint">{{ t('cron.subtitle') }}</div>
        <button class="feature-window-btn" type="button" :disabled="loading" @click="refreshAll">
          {{ t('common.refresh') }}
        </button>
      </div>
      <div class="feature-window-grid">
        <div class="feature-window-list">
          <div v-if="loading" class="feature-window-empty">{{ t('common.loading') }}</div>
          <div v-else-if="!jobs.length" class="feature-window-empty">{{ t('cron.list.empty') }}</div>
          <button
            v-for="job in jobs"
            :key="job.job_id"
            class="feature-window-item"
            :class="{ active: selectedJobId === job.job_id }"
            type="button"
            @click="selectJob(job)"
          >
            <div class="feature-window-item-title">{{ job.name || job.job_id }}</div>
            <div class="feature-window-item-meta">
              <span>{{ job.enabled ? t('cron.status.enabled') : t('cron.status.disabled') }}</span>
              <span>{{ formatSchedule(job) }}</span>
            </div>
            <div class="feature-window-item-sub">
              {{ job.next_run_at_text || t('cron.status.noNext') }}
            </div>
          </button>
        </div>
        <div class="feature-window-detail">
          <div v-if="!selectedJob" class="feature-window-empty">{{ t('cron.detail.empty') }}</div>
          <template v-else>
            <div class="feature-window-item-title">{{ selectedJob.name || selectedJob.job_id }}</div>
            <div class="feature-window-kv">
              <div>{{ t('cron.detail.schedule') }}</div>
              <div>{{ formatSchedule(selectedJob) }}</div>
            </div>
            <div class="feature-window-kv">
              <div>{{ t('cron.detail.payload') }}</div>
              <div class="feature-window-break">{{ payloadText }}</div>
            </div>
            <div class="feature-window-kv">
              <div>{{ t('cron.detail.nextRun') }}</div>
              <div>{{ selectedJob.next_run_at_text || t('cron.status.noNext') }}</div>
            </div>
            <div class="feature-window-actions">
              <button class="feature-window-btn" type="button" @click="toggleEnable">
                {{ selectedJob.enabled ? t('common.disable') : t('common.enable') }}
              </button>
              <button class="feature-window-btn" type="button" @click="runSelectedJob">
                {{ t('cron.action.run') }}
              </button>
              <button class="feature-window-btn danger" type="button" @click="removeSelectedJob">
                {{ t('common.delete') }}
              </button>
            </div>
            <div class="feature-window-runs-title">{{ t('cron.runs.title') }}</div>
            <div v-if="runsLoading" class="feature-window-empty">{{ t('common.loading') }}</div>
            <div v-else-if="!runs.length" class="feature-window-empty">{{ t('cron.runs.empty') }}</div>
            <div v-else class="feature-window-runs-layout">
              <div class="feature-window-runs">
                <button
                  v-for="run in runs"
                  :key="run.run_id"
                  class="feature-window-run-item"
                  :class="{ active: selectedRunId === run.run_id }"
                  type="button"
                  @click="selectedRunId = run.run_id"
                >
                  <div class="feature-window-run-head">
                    <span>{{ formatRunStatus(run.status) }}</span>
                    <span>{{ formatRunTime(run) }}</span>
                  </div>
                  <div class="feature-window-run-summary">
                    {{ run.summary || run.error || t('common.none') }}
                  </div>
                </button>
              </div>
              <div class="feature-window-run-detail">
                <div v-if="!selectedRun" class="feature-window-empty">
                  {{ t('cron.run.detail.empty') }}
                </div>
                <template v-else>
                  <div class="feature-window-run-detail-title">{{ t('cron.run.detail.title') }}</div>
                  <div class="feature-window-kv">
                    <div>{{ t('cron.run.detail.status') }}</div>
                    <div>{{ formatRunStatus(selectedRun.status) }}</div>
                  </div>
                  <div class="feature-window-kv">
                    <div>{{ t('cron.run.detail.trigger') }}</div>
                    <div>{{ formatRunTrigger(selectedRun.trigger) }}</div>
                  </div>
                  <div class="feature-window-kv">
                    <div>{{ t('cron.run.detail.createdAt') }}</div>
                    <div>{{ formatRunTime(selectedRun) }}</div>
                  </div>
                  <div class="feature-window-kv">
                    <div>{{ t('cron.run.detail.duration') }}</div>
                    <div>{{ formatDuration(selectedRun.duration_ms) }}</div>
                  </div>
                  <div class="feature-window-kv">
                    <div>{{ t('cron.run.detail.id') }}</div>
                    <div class="feature-window-break">{{ selectedRun.run_id || t('common.none') }}</div>
                  </div>
                  <div class="feature-window-kv">
                    <div>{{ t('cron.run.detail.summary') }}</div>
                    <div class="feature-window-break">{{ selectedRun.summary || t('common.none') }}</div>
                  </div>
                  <div class="feature-window-kv">
                    <div>{{ t('cron.run.detail.error') }}</div>
                    <div class="feature-window-break">{{ selectedRun.error || t('common.none') }}</div>
                  </div>
                </template>
              </div>
            </div>
          </template>
        </div>
      </div>
    </div>
  </el-dialog>
</template>

<script setup>
import { computed, ref, watch } from 'vue';
import { ElMessageBox } from 'element-plus';

import { fetchCronJobs, fetchCronRuns, disableCronJob, enableCronJob, removeCronJob, runCronJob } from '@/api/cron';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

const props = defineProps({
  modelValue: {
    type: Boolean,
    default: false
  },
  agentId: {
    type: String,
    default: ''
  }
});

const emit = defineEmits(['update:modelValue']);
const { t } = useI18n();

const visible = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value)
});

const contextAgentId = computed(() => {
  const value = String(props.agentId || '').trim();
  if (!value || value === '__default__' || value === 'default') {
    return '';
  }
  return value;
});

const jobs = ref([]);
const runs = ref([]);
const loading = ref(false);
const runsLoading = ref(false);
const selectedJobId = ref('');
const selectedRunId = ref('');

const selectedJob = computed(() => jobs.value.find((job) => job.job_id === selectedJobId.value) || null);
const selectedRun = computed(() => runs.value.find((run) => run.run_id === selectedRunId.value) || null);
const payloadText = computed(() => {
  const payload = selectedJob.value?.payload;
  if (!payload) return t('common.none');
  if (typeof payload === 'string') return payload;
  if (payload.message) return payload.message;
  try {
    return JSON.stringify(payload, null, 2);
  } catch (error) {
    return String(payload);
  }
});

const resolveError = (error) =>
  error?.response?.data?.detail?.message ||
  error?.response?.data?.detail ||
  error?.message ||
  t('cron.action.failed');

const formatTime = (value) => {
  if (!value) return '';
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) {
    return String(value);
  }
  const pad = (part) => String(part).padStart(2, '0');
  return (
    String(parsed.getFullYear()) +
    '-' +
    pad(parsed.getMonth() + 1) +
    '-' +
    pad(parsed.getDate()) +
    ' ' +
    pad(parsed.getHours()) +
    ':' +
    pad(parsed.getMinutes())
  );
};


const formatSchedule = (job) => {
  const schedule = job?.schedule || {};
  if (schedule.kind === 'every') {
    return t('cron.schedule.every', { value: schedule.every_ms || '-' });
  }
  if (schedule.kind === 'cron') {
    return [schedule.cron, schedule.tz].filter(Boolean).join(' ');
  }
  if (schedule.kind === 'at') {
    return schedule.at || '-';
  }
  return '-';
};

const formatRunStatus = (status) => {
  if (status === 'ok') return t('cron.run.status.ok');
  if (status === 'error') return t('cron.run.status.error');
  if (status === 'skipped') return t('cron.run.status.skipped');
  return status || t('common.unknown');
};

const formatRunTime = (run) => run?.created_at_text || formatTime(run?.created_at) || '-';

const formatDuration = (durationMs) => {
  if (durationMs === null || durationMs === undefined || Number.isNaN(Number(durationMs))) {
    return '-';
  }
  return String(Number(durationMs)) + ' ms';
};

const formatRunTrigger = (trigger) => {
  if (trigger === 'manual') return t('cron.run.trigger.manual');
  if (trigger === 'schedule') return t('cron.run.trigger.schedule');
  if (trigger === 'api') return t('cron.run.trigger.api');
  return trigger || t('common.unknown');
};

const loadJobs = async () => {
  loading.value = true;
  try {
    const params = contextAgentId.value ? { agent_id: contextAgentId.value } : undefined;
    const { data } = await fetchCronJobs(params);
    const items = Array.isArray(data?.data?.jobs) ? data.data.jobs : [];
    jobs.value = items;
    if (!items.length) {
      selectedJobId.value = '';
      selectedRunId.value = '';
      runs.value = [];
      return;
    }
    if (!selectedJobId.value || !items.find((job) => job.job_id === selectedJobId.value)) {
      selectedJobId.value = items[0].job_id;
    }
  } catch (error) {
    showApiError(error, resolveError(error));
  } finally {
    loading.value = false;
  }
};

const loadRuns = async (jobId) => {
  if (!jobId) {
    selectedRunId.value = '';
    runs.value = [];
    return;
  }
  runsLoading.value = true;
  try {
    const params = contextAgentId.value ? { agent_id: contextAgentId.value } : undefined;
    const { data } = await fetchCronRuns(jobId, params);
    const items = Array.isArray(data?.data?.runs) ? data.data.runs : [];
    runs.value = items;
    if (!items.length) {
      selectedRunId.value = '';
      return;
    }
    if (!selectedRunId.value || !items.find((run) => run.run_id === selectedRunId.value)) {
      selectedRunId.value = items[0].run_id;
    }
  } catch (error) {
    showApiError(error, resolveError(error));
  } finally {
    runsLoading.value = false;
  }
};

const refreshAll = async () => {
  await loadJobs();
  await loadRuns(selectedJobId.value);
};

const selectJob = async (job) => {
  selectedJobId.value = job.job_id;
  selectedRunId.value = '';
  await loadRuns(job.job_id);
};

const buildJobPayload = (job) => {
  const payload = { job_id: job.job_id };
  const agentId = String(job?.agent_id || '').trim() || contextAgentId.value;
  if (agentId && agentId !== '__default__' && agentId !== 'default') {
    payload.agent_id = agentId;
  }
  return payload;
};

const toggleEnable = async () => {
  const job = selectedJob.value;
  if (!job) return;
  try {
    const payload = {
      action: job.enabled ? 'disable' : 'enable',
      job: buildJobPayload(job)
    };
    if (job.enabled) {
      await disableCronJob(payload);
    } else {
      await enableCronJob(payload);
    }
    await refreshAll();
  } catch (error) {
    showApiError(error, resolveError(error));
  }
};

const runSelectedJob = async () => {
  const job = selectedJob.value;
  if (!job) return;
  try {
    await ElMessageBox.confirm(t('cron.action.confirmRun'), t('common.notice'), {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
    await runCronJob({ action: 'run', job: buildJobPayload(job) });
    await refreshAll();
  } catch (error) {
    if (error === 'cancel' || error === 'close') return;
    showApiError(error, resolveError(error));
  }
};

const removeSelectedJob = async () => {
  const job = selectedJob.value;
  if (!job) return;
  try {
    await ElMessageBox.confirm(t('cron.action.confirmDelete'), t('common.notice'), {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
    await removeCronJob({ action: 'remove', job: buildJobPayload(job) });
    await refreshAll();
  } catch (error) {
    if (error === 'cancel' || error === 'close') return;
    showApiError(error, resolveError(error));
  }
};

watch(
  () => visible.value,
  (value) => {
    if (value) {
      refreshAll();
    }
  }
);

watch(
  () => contextAgentId.value,
  () => {
    if (visible.value) {
      refreshAll();
    }
  }
);
</script>

<style scoped>
:global(.feature-window-dialog--cron.el-dialog) {
  --fw-text: #e2e8f0;
  --fw-muted: #94a3b8;
  --fw-bg: linear-gradient(160deg, #070d1a, #0b1426);
  --fw-shadow: 0 24px 56px rgba(8, 12, 24, 0.55);
  --fw-border: rgba(51, 65, 85, 0.72);
  --fw-border-soft: rgba(51, 65, 85, 0.62);
  --fw-divider: rgba(51, 65, 85, 0.62);
  --fw-surface: #0b1527;
  --fw-surface-alt: #0d182c;
  --fw-control-bg: #111c31;
  --fw-control-hover: #162844;
  --fw-focus-border: rgba(56, 189, 248, 0.65);
  --fw-focus-ring: rgba(56, 189, 248, 0.18);
  --fw-accent-border: rgba(77, 216, 255, 0.65);
  --fw-accent-shadow: rgba(77, 216, 255, 0.35);
  --fw-danger: #fca5a5;
  --fw-danger-border: rgba(248, 113, 113, 0.4);
  width: min(96vw, 1080px) !important;
  max-width: 1080px;
  height: min(82vh, 760px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--fw-bg);
  border: 1px solid var(--fw-border);
  border-radius: 14px;
  box-shadow: var(--fw-shadow);
  color: var(--fw-text);
  color-scheme: dark;
}

:global(:root[data-user-theme='light'] .feature-window-dialog--cron.el-dialog) {
  --fw-text: #0f172a;
  --fw-muted: #64748b;
  --fw-bg: linear-gradient(180deg, #ffffff, #f7faff);
  --fw-shadow: 0 18px 40px rgba(15, 23, 42, 0.16);
  --fw-border: rgba(148, 163, 184, 0.52);
  --fw-border-soft: rgba(148, 163, 184, 0.36);
  --fw-divider: rgba(148, 163, 184, 0.42);
  --fw-surface: #f8fafc;
  --fw-surface-alt: #ffffff;
  --fw-control-bg: #f1f5f9;
  --fw-control-hover: #e2e8f0;
  --fw-focus-border: rgba(37, 99, 235, 0.55);
  --fw-focus-ring: rgba(37, 99, 235, 0.16);
  --fw-accent-border: rgba(37, 99, 235, 0.42);
  --fw-accent-shadow: rgba(37, 99, 235, 0.22);
  --fw-danger: #b91c1c;
  --fw-danger-border: rgba(220, 38, 38, 0.32);
  color-scheme: light;
}

:global(.feature-window-dialog--cron .el-dialog__header) {
  border-bottom: 1px solid var(--fw-divider);
  padding: 14px 18px;
}

:global(.feature-window-dialog--cron .el-dialog__body) {
  padding: 16px 18px 18px;
  color: var(--fw-text);
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.feature-window-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.feature-window-title {
  font-size: 15px;
  font-weight: 700;
}

.feature-window-close {
  width: 30px;
  height: 30px;
  border: 1px solid var(--fw-border);
  border-radius: 10px;
  background: var(--fw-control-bg);
  color: var(--fw-text);
  cursor: pointer;
}

.feature-window-close:hover {
  border-color: var(--fw-focus-border);
  background: var(--fw-control-hover);
}

.feature-window-close:focus-visible {
  outline: 2px solid var(--fw-focus-ring);
  outline-offset: 1px;
}

.feature-window-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.feature-window-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.feature-window-hint {
  color: var(--fw-muted);
  font-size: 12px;
}

.feature-window-grid {
  display: grid;
  grid-template-columns: minmax(280px, 320px) minmax(0, 1fr);
  gap: 14px;
  flex: 1;
  min-height: 0;
}

.feature-window-list {
  max-height: none;
  min-height: 0;
  overflow: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.feature-window-item {
  border: 1px solid var(--fw-border-soft);
  border-radius: 10px;
  background: var(--fw-surface-alt);
  color: var(--fw-text);
  padding: 10px;
  text-align: left;
  display: flex;
  flex-direction: column;
  gap: 4px;
  cursor: pointer;
}

.feature-window-item.active {
  border-color: var(--fw-accent-border);
  box-shadow: inset 0 0 0 1px var(--fw-accent-shadow);
}

.feature-window-item-title {
  font-size: 13px;
  font-weight: 700;
}

.feature-window-item-meta {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  font-size: 12px;
  color: var(--fw-muted);
}

.feature-window-item-sub {
  font-size: 12px;
  color: var(--fw-muted);
}

.feature-window-detail {
  border: 1px solid var(--fw-border-soft);
  border-radius: 10px;
  background: var(--fw-surface);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  overflow: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
}

.feature-window-kv {
  display: grid;
  grid-template-columns: 96px minmax(0, 1fr);
  gap: 8px;
  font-size: 12px;
}

.feature-window-break {
  word-break: break-word;
  white-space: pre-wrap;
}

.feature-window-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.feature-window-btn {
  border: 1px solid var(--fw-border);
  border-radius: 10px;
  background: var(--fw-control-bg);
  color: var(--fw-text);
  padding: 6px 10px;
  font-size: 12px;
  cursor: pointer;
}

.feature-window-btn:hover {
  border-color: var(--fw-focus-border);
  background: var(--fw-control-hover);
  color: var(--fw-text);
}

.feature-window-btn:focus-visible {
  outline: 2px solid var(--fw-focus-ring);
  outline-offset: 1px;
}

.feature-window-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.feature-window-btn.danger {
  border-color: var(--fw-danger-border);
  color: var(--fw-danger);
}

.feature-window-runs-title {
  margin-top: 2px;
  font-size: 12px;
  color: var(--fw-muted);
}

.feature-window-runs-layout {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(240px, 320px);
  gap: 10px;
  min-height: 0;
  flex: 1;
}

.feature-window-runs {
  display: flex;
  flex-direction: column;
  gap: 6px;
  height: 100%;
  min-height: 0;
  overflow: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
}

.feature-window-run-item {
  border: 1px solid var(--fw-border-soft);
  border-radius: 8px;
  padding: 8px;
  background: var(--fw-surface-alt);
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
  color: var(--fw-text);
  text-align: left;
  cursor: pointer;
}

.feature-window-run-item.active {
  border-color: var(--fw-accent-border);
  box-shadow: inset 0 0 0 1px var(--fw-accent-shadow);
}

.feature-window-run-head {
  display: flex;
  justify-content: space-between;
  gap: 8px;
  color: var(--fw-muted);
}

.feature-window-run-summary {
  color: var(--fw-text);
  white-space: pre-wrap;
  word-break: break-word;
  line-height: 1.35;
}

.feature-window-run-detail {
  border: 1px solid var(--fw-border-soft);
  border-radius: 8px;
  padding: 8px;
  background: var(--fw-surface-alt);
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  overflow: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
}

.feature-window-run-detail-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--fw-muted);
}

.feature-window-empty {
  color: var(--fw-muted);
  font-size: 12px;
  text-align: center;
  padding: 12px;
}

@media (max-width: 900px) {
  .feature-window-grid {
    grid-template-columns: 1fr;
  }

  .feature-window-list {
    max-height: 30vh;
  }

  .feature-window-runs-layout {
    grid-template-columns: 1fr;
  }

  .feature-window-runs {
    max-height: 20vh;
  }
}
</style>
