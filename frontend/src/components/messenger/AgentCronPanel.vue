<template>
  <div class="messenger-cron-panel">
    <div class="messenger-cron-toolbar">
      <div class="messenger-inline-hint">{{ t('cron.subtitle') }}</div>
      <button class="messenger-inline-btn" type="button" :disabled="loading || creating" @click="refreshAll">
        {{ t('common.refresh') }}
      </button>
    </div>

    <div class="messenger-cron-create">
      <div class="messenger-cron-create-title">{{ t('cron.create.title') }}</div>
      <label class="messenger-cron-field">
        <span>{{ t('cron.create.message') }}</span>
        <textarea
          v-model="createForm.message"
          rows="3"
          :placeholder="t('cron.create.messagePlaceholder')"
        ></textarea>
      </label>
      <label class="messenger-cron-field">
        <span>{{ t('cron.create.runAt') }}</span>
        <input v-model="createForm.runAt" type="datetime-local" />
      </label>
      <div class="messenger-cron-mode-row">
        <span>{{ t('cron.create.mode') }}</span>
        <button
          class="messenger-inline-btn"
          :class="{ active: createForm.mode === 'once' }"
          type="button"
          @click="createForm.mode = 'once'"
        >
          {{ t('cron.create.mode.once') }}
        </button>
        <button
          class="messenger-inline-btn"
          :class="{ active: createForm.mode === 'repeat' }"
          type="button"
          @click="createForm.mode = 'repeat'"
        >
          {{ t('cron.create.mode.repeat') }}
        </button>
      </div>
      <div v-if="createForm.mode === 'repeat'" class="messenger-cron-repeat-row">
        <span>{{ t('cron.create.intervalEvery') }}</span>
        <input
          v-model.number="createForm.intervalValue"
          type="number"
          min="1"
          step="1"
        />
        <select v-model="createForm.intervalUnit">
          <option value="minute">{{ t('cron.create.interval.unit.minute') }}</option>
          <option value="hour">{{ t('cron.create.interval.unit.hour') }}</option>
          <option value="day">{{ t('cron.create.interval.unit.day') }}</option>
        </select>
      </div>
      <div class="messenger-cron-actions">
        <button class="messenger-inline-btn" type="button" :disabled="creating" @click="resetCreateForm">
          {{ t('common.reset') }}
        </button>
        <button class="messenger-inline-btn primary" type="button" :disabled="creating" @click="createJob">
          {{ creating ? t('common.loading') : t('cron.create.submit') }}
        </button>
      </div>
    </div>

    <div class="messenger-cron-body">
      <div class="messenger-cron-list">
        <div v-if="loading" class="messenger-list-empty">{{ t('common.loading') }}</div>
        <div v-else-if="!jobs.length" class="messenger-list-empty">{{ t('cron.list.empty') }}</div>
        <button
          v-for="job in jobs"
          :key="job.job_id"
          class="messenger-list-item"
          :class="{ active: selectedJobId === job.job_id }"
          type="button"
          @click="selectJob(job)"
        >
          <div class="messenger-list-main">
            <div class="messenger-list-row">
              <span class="messenger-list-name">{{ job.name || job.job_id }}</span>
            </div>
            <div class="messenger-list-row">
              <span class="messenger-list-preview">{{ formatSchedule(job) }}</span>
              <span class="messenger-kind-tag">{{ job.enabled ? t('cron.status.enabled') : t('cron.status.disabled') }}</span>
            </div>
          </div>
        </button>
      </div>

      <div class="messenger-cron-detail">
        <div v-if="!selectedJob" class="messenger-list-empty">{{ t('cron.detail.empty') }}</div>
        <template v-else>
          <div class="messenger-cron-title">{{ selectedJob.name || selectedJob.job_id }}</div>
          <div class="messenger-cron-kv">
            <span>{{ t('cron.detail.schedule') }}</span>
            <span>{{ formatSchedule(selectedJob) }}</span>
          </div>
          <div class="messenger-cron-kv">
            <span>{{ t('cron.detail.payload') }}</span>
            <span class="messenger-cron-break">{{ payloadText }}</span>
          </div>
          <div class="messenger-cron-kv">
            <span>{{ t('cron.detail.nextRun') }}</span>
            <span>{{ selectedJob.next_run_at_text || t('cron.status.noNext') }}</span>
          </div>
          <div class="messenger-cron-actions">
            <button class="messenger-inline-btn" type="button" @click="toggleEnable">
              {{ selectedJob.enabled ? t('common.disable') : t('common.enable') }}
            </button>
            <button class="messenger-inline-btn" type="button" @click="runSelectedJob">
              {{ t('cron.action.run') }}
            </button>
            <button class="messenger-inline-btn danger" type="button" @click="removeSelectedJob">
              {{ t('common.delete') }}
            </button>
          </div>

          <div class="messenger-cron-run-title">{{ t('cron.runs.title') }}</div>
          <div v-if="runsLoading" class="messenger-list-empty">{{ t('common.loading') }}</div>
          <div v-else-if="!runs.length" class="messenger-list-empty">{{ t('cron.runs.empty') }}</div>
          <div v-else class="messenger-cron-runs">
            <button
              v-for="run in runs"
              :key="run.run_id"
              class="messenger-cron-run-item"
              :class="{ active: selectedRunId === run.run_id }"
              type="button"
              @click="selectedRunId = run.run_id"
            >
              <div class="messenger-list-row">
                <span class="messenger-list-name">{{ formatRunStatus(run.status) }}</span>
                <span class="messenger-list-time">{{ formatRunTime(run) }}</span>
              </div>
              <div class="messenger-list-preview">{{ run.summary || run.error || t('common.none') }}</div>
            </button>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import {
  addCronJob,
  fetchCronJobs,
  fetchCronRuns,
  disableCronJob,
  enableCronJob,
  removeCronJob,
  runCronJob
} from '@/api/cron';
import { createSession, listSessions } from '@/api/chat';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

const props = defineProps({
  agentId: {
    type: String,
    default: ''
  }
});

const { t } = useI18n();

const contextAgentId = computed(() => {
  const value = String(props.agentId || '').trim();
  if (!value || value === '__default__' || value === 'default') return '';
  return value;
});

const jobs = ref<any[]>([]);
const runs = ref<any[]>([]);
const loading = ref(false);
const runsLoading = ref(false);
const creating = ref(false);
const selectedJobId = ref('');
const selectedRunId = ref('');

const createForm = reactive({
  message: '',
  runAt: '',
  mode: 'once',
  intervalValue: 5,
  intervalUnit: 'minute'
});

const INTERVAL_UNIT_MS: Record<string, number> = {
  minute: 60 * 1000,
  hour: 60 * 60 * 1000,
  day: 24 * 60 * 60 * 1000
};

const selectedJob = computed(() => jobs.value.find((job) => job.job_id === selectedJobId.value) || null);
const payloadText = computed(() => {
  const payload = selectedJob.value?.payload;
  if (!payload) return t('common.none');
  if (typeof payload === 'string') return payload;
  if (payload.message) return payload.message;
  try {
    return JSON.stringify(payload, null, 2);
  } catch {
    return String(payload);
  }
});

const resolveError = (error: unknown): string => {
  const source = error as {
    response?: { data?: { detail?: { message?: string } | string } };
    message?: string;
  };
  return String(
    source?.response?.data?.detail && typeof source.response.data.detail === 'object'
      ? source.response.data.detail.message
      : source?.response?.data?.detail || source?.message || t('cron.action.failed')
  );
};

const toDateTimeLocalValue = (date: Date): string => {
  if (!(date instanceof Date) || Number.isNaN(date.getTime())) return '';
  const pad = (part: number) => String(part).padStart(2, '0');
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

const formatSchedule = (job: Record<string, unknown>): string => {
  const schedule = (job?.schedule || {}) as Record<string, unknown>;
  if (schedule.kind === 'every') {
    return t('cron.schedule.every', { value: String(schedule.every_ms || '-') });
  }
  if (schedule.kind === 'cron') {
    return [String(schedule.cron || ''), String(schedule.tz || '')].filter(Boolean).join(' ');
  }
  if (schedule.kind === 'at') {
    return String(schedule.at || '-');
  }
  return '-';
};

const formatRunStatus = (status: unknown): string => {
  const value = String(status || '');
  if (value === 'ok') return t('cron.run.status.ok');
  if (value === 'error') return t('cron.run.status.error');
  if (value === 'skipped') return t('cron.run.status.skipped');
  return value || t('common.unknown');
};

const formatRunTime = (run: Record<string, unknown>): string =>
  String(run?.created_at_text || run?.created_at || '-');

const resolveIntervalMs = (): number | null => {
  const intervalValue = Number.parseInt(String(createForm.intervalValue), 10);
  if (!Number.isFinite(intervalValue) || intervalValue <= 0) return null;
  const unitMs = INTERVAL_UNIT_MS[createForm.intervalUnit] || INTERVAL_UNIT_MS.minute;
  return intervalValue * unitMs;
};

const resolveTargetSessionId = async (): Promise<string> => {
  const agentId = contextAgentId.value;
  const params = agentId ? { agent_id: agentId } : undefined;
  const { data } = await listSessions(params);
  const items = Array.isArray(data?.data?.items) ? data.data.items : [];
  const candidate = items.find((item: Record<string, unknown>) => item?.is_main) || items[0];
  const sessionId = String(candidate?.id || '').trim();
  if (sessionId) return sessionId;
  const created = await createSession(agentId ? { agent_id: agentId } : {});
  const createdSessionId = String(created?.data?.data?.id || '').trim();
  if (createdSessionId) return createdSessionId;
  throw new Error(t('error.session_not_found'));
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
    if (!selectedJobId.value || !items.find((job: Record<string, unknown>) => job.job_id === selectedJobId.value)) {
      selectedJobId.value = String(items[0].job_id || '');
    }
  } catch (error) {
    showApiError(error, resolveError(error));
  } finally {
    loading.value = false;
  }
};

const loadRuns = async (jobId: string) => {
  if (!jobId) {
    selectedRunId.value = '';
    runs.value = [];
    return;
  }
  runsLoading.value = true;
  try {
    const params = contextAgentId.value ? { agent_id: contextAgentId.value } : undefined;
    const { data } = await fetchCronRuns(jobId, params || {});
    const items = Array.isArray(data?.data?.runs) ? data.data.runs : [];
    runs.value = items;
    if (!items.length) {
      selectedRunId.value = '';
      return;
    }
    if (!selectedRunId.value || !items.find((run: Record<string, unknown>) => run.run_id === selectedRunId.value)) {
      selectedRunId.value = String(items[0].run_id || '');
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

const selectJob = async (job: Record<string, unknown>) => {
  selectedJobId.value = String(job.job_id || '');
  selectedRunId.value = '';
  await loadRuns(selectedJobId.value);
};

const buildJobPayload = (job: Record<string, unknown>) => {
  const payload: Record<string, string> = { job_id: String(job.job_id || '') };
  const agentId = String(job?.agent_id || '').trim() || contextAgentId.value;
  if (agentId && agentId !== '__default__' && agentId !== 'default') {
    payload.agent_id = agentId;
  }
  return payload;
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

  let schedule: Record<string, unknown> = {
    kind: 'at',
    at: runAt.toISOString()
  };
  if (createForm.mode === 'repeat') {
    const everyMs = resolveIntervalMs();
    if (!Number.isFinite(everyMs) || (everyMs as number) <= 0) {
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
    const agentId = contextAgentId.value;
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
    resetCreateForm();
    await refreshAll();
  } catch (error) {
    showApiError(error, resolveError(error));
  } finally {
    creating.value = false;
  }
};

const toggleEnable = async () => {
  const job = selectedJob.value as Record<string, unknown> | null;
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
  const job = selectedJob.value as Record<string, unknown> | null;
  if (!job) return;
  try {
    await runCronJob({ action: 'run', job: buildJobPayload(job) });
    await refreshAll();
  } catch (error) {
    showApiError(error, resolveError(error));
  }
};

const removeSelectedJob = async () => {
  const job = selectedJob.value as Record<string, unknown> | null;
  if (!job) return;
  try {
    await removeCronJob({ action: 'remove', job: buildJobPayload(job) });
    await refreshAll();
  } catch (error) {
    showApiError(error, resolveError(error));
  }
};

watch(
  () => contextAgentId.value,
  async () => {
    resetCreateForm();
    await refreshAll();
  },
  { immediate: true }
);
</script>
