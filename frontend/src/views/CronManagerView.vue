<template>
  <div class="portal-shell cron-manager-shell">
    <UserTopbar :title="t('cron.title')" :subtitle="t('cron.subtitle')" :hide-chat="true" />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <section class="portal-section cron-manager-section">
            <div class="cron-manager-page user-tools-dialog">
              <div class="cron-sidebar">
                <div class="cron-sidebar-header">
                  <div class="cron-sidebar-title">{{ t('cron.list.title') }}</div>
                  <div class="cron-sidebar-actions">
                    <button class="cron-refresh-btn" type="button" @click="refreshAll" :disabled="loading">
                      {{ t('common.refresh') }}
                    </button>
                    <button
                      class="cron-refresh-btn"
                      type="button"
                      :disabled="creating"
                      @click="toggleCreatePanel"
                    >
                      {{ createPanelVisible ? t('cron.create.cancel') : t('cron.action.create') }}
                    </button>
                  </div>
                </div>
                <div v-if="createPanelVisible" class="cron-create-panel">
                  <div class="cron-create-title">{{ t('cron.create.title') }}</div>
                  <label class="cron-create-label" for="cron-create-message">{{ t('cron.create.message') }}</label>
                  <textarea
                    id="cron-create-message"
                    v-model="createForm.message"
                    class="cron-create-input cron-create-textarea"
                    :placeholder="t('cron.create.messagePlaceholder')"
                  ></textarea>

                  <label class="cron-create-label" for="cron-create-run-at">{{ t('cron.create.runAt') }}</label>
                  <input
                    id="cron-create-run-at"
                    v-model="createForm.runAt"
                    class="cron-create-input"
                    type="datetime-local"
                  />

                  <label class="cron-create-label">{{ t('cron.create.mode') }}</label>
                  <div class="cron-create-mode">
                    <button
                      class="cron-create-mode-btn"
                      :class="{ active: createForm.mode === 'once' }"
                      type="button"
                      @click="createForm.mode = 'once'"
                    >
                      {{ t('cron.create.mode.once') }}
                    </button>
                    <button
                      class="cron-create-mode-btn"
                      :class="{ active: createForm.mode === 'repeat' }"
                      type="button"
                      @click="createForm.mode = 'repeat'"
                    >
                      {{ t('cron.create.mode.repeat') }}
                    </button>
                  </div>

                  <div v-if="createForm.mode === 'repeat'" class="cron-create-interval">
                    <span>{{ t('cron.create.intervalEvery') }}</span>
                    <input
                      v-model.number="createForm.intervalValue"
                      class="cron-create-input cron-create-input--number"
                      type="number"
                      min="1"
                      step="1"
                    />
                    <select v-model="createForm.intervalUnit" class="cron-create-input cron-create-input--select">
                      <option value="minute">{{ t('cron.create.interval.unit.minute') }}</option>
                      <option value="hour">{{ t('cron.create.interval.unit.hour') }}</option>
                      <option value="day">{{ t('cron.create.interval.unit.day') }}</option>
                    </select>
                  </div>

                  <div class="cron-create-hint">{{ t('cron.create.hint') }}</div>
                  <div class="cron-create-actions">
                    <button class="cron-action-btn" type="button" :disabled="creating" @click="resetCreateForm">
                      {{ t('common.reset') }}
                    </button>
                    <button class="cron-action-btn" type="button" :disabled="creating" @click="createJob">
                      {{ creating ? t('common.loading') : t('cron.create.submit') }}
                    </button>
                  </div>
                </div>
                <div v-if="loading" class="cron-empty">{{ t('common.loading') }}</div>
                <div v-else-if="!jobs.length" class="cron-empty">{{ t('cron.list.empty') }}</div>
                <div v-else class="cron-job-list">
                  <button
                    v-for="job in jobs"
                    :key="job.job_id"
                    class="cron-job-card"
                    :class="{ active: job.job_id === selectedJobId }"
                    type="button"
                    @click="selectJob(job)"
                  >
                    <div class="cron-job-title">{{ job.name || job.job_id }}</div>
                    <div class="cron-job-meta">
                      <span class="cron-job-tag">
                        {{ job.enabled ? t('cron.status.enabled') : t('cron.status.disabled') }}
                      </span>
                      <span class="cron-job-tag">{{ job.schedule?.kind || '-' }}</span>
                    </div>
                    <div class="cron-job-next">
                      {{ job.next_run_at_text || t('cron.status.noNext') }}
                    </div>
                  </button>
                </div>
              </div>
              <div class="cron-content">
                <div class="cron-content-header">
                  <div class="cron-content-title">
                    {{ selectedJob?.name || selectedJob?.job_id || t('cron.detail.title') }}
                  </div>
                  <div class="cron-actions">
                    <button
                      class="cron-action-btn"
                      type="button"
                      :disabled="!selectedJob"
                      @click="toggleEnable"
                    >
                      {{ selectedJob?.enabled ? t('common.disable') : t('common.enable') }}
                    </button>
                    <button
                      class="cron-action-btn"
                      type="button"
                      :disabled="!selectedJob"
                      @click="runJob"
                    >
                      {{ t('cron.action.run') }}
                    </button>
                    <button
                      class="cron-action-btn danger"
                      type="button"
                      :disabled="!selectedJob"
                      @click="deleteJob"
                    >
                      {{ t('common.delete') }}
                    </button>
                  </div>
                </div>
                <div v-if="!selectedJob" class="cron-empty">{{ t('cron.detail.empty') }}</div>
                <table v-else class="cron-detail-table">
                  <tbody>
                    <tr>
                      <th>{{ t('cron.detail.schedule') }}</th>
                      <td class="cron-detail-value">{{ formatSchedule(selectedJob) }}</td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.payload') }}</th>
                      <td class="cron-detail-value pre">{{ payloadMessage }}</td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.nextRun') }}</th>
                      <td class="cron-detail-value">
                        {{ selectedJob.next_run_at_text || t('cron.status.noNext') }}
                      </td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.lastRun') }}</th>
                      <td class="cron-detail-value">
                        {{ selectedJob.last_run_at_text || t('common.none') }}
                      </td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.status') }}</th>
                      <td class="cron-detail-value">
                        {{ selectedJob.last_status || t('common.unknown') }}
                      </td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.error') }}</th>
                      <td class="cron-detail-value pre">
                        {{ selectedJob.last_error || t('common.none') }}
                      </td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.session') }}</th>
                      <td class="cron-detail-value">{{ selectedJob.session_id || t('common.none') }}</td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.agent') }}</th>
                      <td class="cron-detail-value">{{ agentIdText }}</td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.target') }}</th>
                      <td class="cron-detail-value">
                        {{ selectedJob.session_target || t('common.none') }}
                      </td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.enabled') }}</th>
                      <td class="cron-detail-value">
                        {{ selectedJob.enabled ? t('common.yes') : t('common.no') }}
                      </td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.deleteAfterRun') }}</th>
                      <td class="cron-detail-value">
                        {{ selectedJob.delete_after_run ? t('common.yes') : t('common.no') }}
                      </td>
                    </tr>
                    <tr>
                      <th>{{ t('cron.detail.dedupeKey') }}</th>
                      <td class="cron-detail-value">{{ selectedJob.dedupe_key || t('common.none') }}</td>
                    </tr>
                  </tbody>
                </table>
                <div class="cron-runs">
                  <div class="list-header">
                    <label>{{ t('cron.runs.title') }}</label>
                    <button
                      class="cron-refresh-btn subtle"
                      type="button"
                      :disabled="runsLoading || !selectedJob"
                      @click="refreshRuns"
                    >
                      {{ t('common.refresh') }}
                    </button>
                  </div>
                  <div v-if="runsLoading" class="cron-empty">{{ t('common.loading') }}</div>
                  <div v-else-if="!runs.length" class="cron-empty">{{ t('cron.runs.empty') }}</div>
                  <div v-else class="cron-run-layout">
                    <div class="cron-run-list">
                      <button
                        v-for="run in runs"
                        :key="run.run_id"
                        class="cron-run-item"
                        :class="{ active: selectedRunId === run.run_id }"
                        type="button"
                        @click="selectedRunId = run.run_id"
                      >
                        <div class="cron-run-main">
                          <div class="cron-run-status">{{ formatRunStatus(run.status) }}</div>
                          <div class="cron-run-time">{{ formatRunTime(run) }}</div>
                        </div>
                        <div class="cron-run-summary">
                          {{ run.summary || run.error || t('common.none') }}
                        </div>
                      </button>
                    </div>
                    <div class="cron-run-detail">
                      <div v-if="!selectedRun" class="cron-empty">{{ t('cron.run.detail.empty') }}</div>
                      <template v-else>
                        <div class="cron-run-detail-title">{{ t('cron.run.detail.title') }}</div>
                        <div class="cron-run-detail-grid">
                          <div class="cron-run-detail-label">{{ t('cron.run.detail.status') }}</div>
                          <div class="cron-run-detail-value">{{ formatRunStatus(selectedRun.status) }}</div>
                          <div class="cron-run-detail-label">{{ t('cron.run.detail.trigger') }}</div>
                          <div class="cron-run-detail-value">{{ formatRunTrigger(selectedRun.trigger) }}</div>
                          <div class="cron-run-detail-label">{{ t('cron.run.detail.createdAt') }}</div>
                          <div class="cron-run-detail-value">{{ formatRunTime(selectedRun) }}</div>
                          <div class="cron-run-detail-label">{{ t('cron.run.detail.duration') }}</div>
                          <div class="cron-run-detail-value">{{ formatDuration(selectedRun.duration_ms) }}</div>
                          <div class="cron-run-detail-label">{{ t('cron.run.detail.id') }}</div>
                          <div class="cron-run-detail-value break">{{ selectedRun.run_id || t('common.none') }}</div>
                          <div class="cron-run-detail-label">{{ t('cron.run.detail.summary') }}</div>
                          <div class="cron-run-detail-value break">{{ selectedRun.summary || t('common.none') }}</div>
                          <div class="cron-run-detail-label">{{ t('cron.run.detail.error') }}</div>
                          <div class="cron-run-detail-value break">{{ selectedRun.error || t('common.none') }}</div>
                        </div>
                      </template>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </section>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup>
import { computed, onMounted, reactive, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  addCronJob,
  fetchCronJobs,
  fetchCronRuns,
  enableCronJob,
  disableCronJob,
  removeCronJob,
  runCronJob
} from '@/api/cron';
import { showApiError } from '@/utils/apiError';
import { createSession, listSessions } from '@/api/chat';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';

const { t } = useI18n();
const route = useRoute();
const jobs = ref([]);
const runs = ref([]);
const loading = ref(false);
const runsLoading = ref(false);
const selectedJobId = ref('');
const selectedRunId = ref('');
const createPanelVisible = ref(false);
const creating = ref(false);
const createForm = reactive({
  message: '',
  runAt: '',
  mode: 'once',
  intervalValue: 5,
  intervalUnit: 'minute'
});

const INTERVAL_UNIT_MS = {
  minute: 60 * 1000,
  hour: 60 * 60 * 1000,
  day: 24 * 60 * 60 * 1000
};

const selectedJob = computed(
  () => jobs.value.find((job) => job.job_id === selectedJobId.value) || null
);
const selectedRun = computed(() => runs.value.find((run) => run.run_id === selectedRunId.value) || null);

const contextAgentId = computed(() => {
  const raw = String(route.query.agent_id || '').trim();
  if (!raw || raw === '__default__' || raw === 'default') {
    return '';
  }
  return raw;
});

const agentIdText = computed(() => {
  const agentId = selectedJob.value?.agent_id;
  const cleaned = typeof agentId === 'string' ? agentId.trim() : '';
  if (cleaned && cleaned !== '__default__' && cleaned !== 'default') {
    return cleaned;
  }
  return t('cron.detail.agentDefault');
});

const payloadMessage = computed(() => {
  const payload = selectedJob.value?.payload;
  if (!payload) {
    return t('common.none');
  }
  if (typeof payload === 'string') {
    return payload;
  }
  if (payload.message) {
    return payload.message;
  }
  try {
    return JSON.stringify(payload, null, 2);
  } catch (error) {
    return String(payload);
  }
});

const resolveErrorMessage = (error) =>
  error?.response?.data?.detail?.message ||
  error?.response?.data?.detail ||
  error?.message ||
  t('cron.action.failed');

const formatTime = (value) => {
  if (!value) {
    return '';
  }
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

const formatRunTime = (run) => run?.created_at_text || formatTime(run?.created_at) || '-';

const formatDuration = (durationMs) => {
  if (durationMs === null || durationMs === undefined || Number.isNaN(Number(durationMs))) {
    return '-';
  }
  return String(Number(durationMs)) + ' ms';
};

const formatRunTrigger = (trigger) => {
  if (trigger === 'manual') {
    return t('cron.run.trigger.manual');
  }
  if (trigger === 'schedule') {
    return t('cron.run.trigger.schedule');
  }
  if (trigger === 'api') {
    return t('cron.run.trigger.api');
  }
  return trigger || t('common.unknown');
};

const toDateTimeLocalValue = (date) => {
  if (!(date instanceof Date) || Number.isNaN(date.getTime())) {
    return '';
  }
  const pad = (part) => String(part).padStart(2, '0');
  return (
    String(date.getFullYear()) +
    '-' +
    pad(date.getMonth() + 1) +
    '-' +
    pad(date.getDate()) +
    'T' +
    pad(date.getHours()) +
    ':' +
    pad(date.getMinutes())
  );
};

const resolveDefaultRunAt = () => toDateTimeLocalValue(new Date(Date.now() + 5 * 60 * 1000));

const resetCreateForm = () => {
  createForm.message = '';
  createForm.runAt = resolveDefaultRunAt();
  createForm.mode = 'once';
  createForm.intervalValue = 5;
  createForm.intervalUnit = 'minute';
};

const toggleCreatePanel = () => {
  createPanelVisible.value = !createPanelVisible.value;
  if (createPanelVisible.value && !createForm.runAt) {
    createForm.runAt = resolveDefaultRunAt();
  }
};

const resolveCreateAgentId = () => {
  const cleaned = String(contextAgentId.value || '').trim();
  if (!cleaned || cleaned === '__default__' || cleaned === 'default') {
    return '';
  }
  return cleaned;
};

const resolveIntervalMs = () => {
  const intervalValue = Number.parseInt(createForm.intervalValue, 10);
  if (!Number.isFinite(intervalValue) || intervalValue <= 0) {
    return null;
  }
  const unitMs = INTERVAL_UNIT_MS[createForm.intervalUnit] || INTERVAL_UNIT_MS.minute;
  return intervalValue * unitMs;
};

const resolveTargetSessionId = async () => {
  const agentId = resolveCreateAgentId();
  const params = agentId ? { agent_id: agentId } : undefined;
  const { data } = await listSessions(params);
  const items = Array.isArray(data?.data?.items) ? data.data.items : [];
  const candidate = items.find((item) => item?.is_main) || items[0];
  const sessionId = String(candidate?.id || '').trim();
  if (sessionId) {
    return sessionId;
  }
  const createPayload = agentId ? { agent_id: agentId } : {};
  const created = await createSession(createPayload);
  const createdSessionId = String(created?.data?.data?.id || '').trim();
  if (createdSessionId) {
    return createdSessionId;
  }
  throw new Error(t('error.session_not_found'));
};

const createJob = async () => {
  const message = String(createForm.message || '').trim();
  if (!message) {
    ElMessage.warning(t('cron.create.messageRequired'));
    return;
  }
  if (!createForm.runAt) {
    ElMessage.warning(t('cron.create.runAtRequired'));
    return;
  }
  const runAt = new Date(createForm.runAt);
  if (Number.isNaN(runAt.getTime())) {
    ElMessage.warning(t('cron.create.runAtInvalid'));
    return;
  }
  let schedule = {
    kind: 'at',
    at: runAt.toISOString()
  };
  if (createForm.mode === 'repeat') {
    const everyMs = resolveIntervalMs();
    if (!Number.isFinite(everyMs) || everyMs <= 0) {
      ElMessage.warning(t('cron.create.intervalInvalid'));
      return;
    }
    schedule = {
      kind: 'every',
      at: runAt.toISOString(),
      every_ms: everyMs
    };
  }
  creating.value = true;
  try {
    const sessionId = await resolveTargetSessionId();
    const agentId = resolveCreateAgentId();
    await addCronJob({
      action: 'add',
      job: {
        session_id: sessionId,
        ...(agentId ? { agent_id: agentId } : {}),
        session: 'main',
        payload: { message },
        schedule,
        enabled: true
      }
    });
    ElMessage.success(t('cron.create.success'));
    createPanelVisible.value = false;
    resetCreateForm();
    await refreshAll();
  } catch (error) {
    showApiError(error, resolveErrorMessage(error));
  } finally {
    creating.value = false;
  }
};

const loadJobs = async () => {
  loading.value = true;
  try {
    const params = contextAgentId.value ? { agent_id: contextAgentId.value } : undefined;
    const { data } = await fetchCronJobs(params);
    const items = data?.data?.jobs || [];
    jobs.value = Array.isArray(items) ? items : [];
    if (!jobs.value.length) {
      selectedJobId.value = '';
      selectedRunId.value = '';
      runs.value = [];
      return;
    }
    if (!selectedJobId.value || !jobs.value.find((job) => job.job_id === selectedJobId.value)) {
      selectedJobId.value = jobs.value[0].job_id;
    }
  } catch (error) {
    showApiError(error, resolveErrorMessage(error));
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
    const items = data?.data?.runs || [];
    runs.value = Array.isArray(items) ? items : [];
    if (!runs.value.length) {
      selectedRunId.value = '';
      return;
    }
    if (!selectedRunId.value || !runs.value.find((run) => run.run_id === selectedRunId.value)) {
      selectedRunId.value = runs.value[0].run_id;
    }
  } catch (error) {
    showApiError(error, resolveErrorMessage(error));
  } finally {
    runsLoading.value = false;
  }
};

const refreshAll = async () => {
  await loadJobs();
  if (selectedJobId.value) {
    await loadRuns(selectedJobId.value);
  }
};

const refreshRuns = async () => {
  if (selectedJobId.value) {
    await loadRuns(selectedJobId.value);
  }
};

const selectJob = async (job) => {
  selectedJobId.value = job.job_id;
  selectedRunId.value = '';
  await loadRuns(job.job_id);
};

const buildJobPayload = (job) => {
  const payload = { job_id: job.job_id };
  const resolvedAgentId =
    String(job?.agent_id || '').trim() || String(contextAgentId.value || '').trim();
  if (resolvedAgentId && resolvedAgentId !== '__default__' && resolvedAgentId !== 'default') {
    payload.agent_id = resolvedAgentId;
  }
  return payload;
};

const toggleEnable = async () => {
  const job = selectedJob.value;
  if (!job) {
    return;
  }
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
    showApiError(error, resolveErrorMessage(error));
  }
};

const runJob = async () => {
  const job = selectedJob.value;
  if (!job) {
    return;
  }
  try {
    await ElMessageBox.confirm(t('cron.action.confirmRun'), t('common.notice'), {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
    await runCronJob({ action: 'run', job: buildJobPayload(job) });
    await refreshAll();
  } catch (error) {
    if (error === 'cancel' || error === 'close') {
      return;
    }
    showApiError(error, resolveErrorMessage(error));
  }
};

const deleteJob = async () => {
  const job = selectedJob.value;
  if (!job) {
    return;
  }
  try {
    await ElMessageBox.confirm(t('cron.action.confirmDelete'), t('common.notice'), {
      confirmButtonText: t('common.confirm'),
      cancelButtonText: t('common.cancel'),
      type: 'warning'
    });
    await removeCronJob({ action: 'remove', job: buildJobPayload(job) });
    await refreshAll();
  } catch (error) {
    if (error === 'cancel' || error === 'close') {
      return;
    }
    showApiError(error, resolveErrorMessage(error));
  }
};

const formatSchedule = (job) => {
  const schedule = job?.schedule || {};
  const kind = schedule.kind;
  if (kind === 'every') {
    return t('cron.schedule.every', { value: schedule.every_ms || '-' });
  }
  if (kind === 'cron') {
    const parts = [schedule.cron, schedule.tz].filter(Boolean);
    return parts.join(' ');
  }
  if (kind === 'at') {
    return schedule.at || '-';
  }
  return '-';
};

const formatRunStatus = (status) => {
  if (status === 'ok') {
    return t('cron.run.status.ok');
  }
  if (status === 'error') {
    return t('cron.run.status.error');
  }
  if (status === 'skipped') {
    return t('cron.run.status.skipped');
  }
  return status || t('common.unknown');
};

onMounted(async () => {
  resetCreateForm();
  await refreshAll();
});

watch(
  () => contextAgentId.value,
  () => {
    refreshAll();
  }
);
</script>