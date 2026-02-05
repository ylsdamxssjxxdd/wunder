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
                  <button class="cron-refresh-btn" type="button" @click="refreshAll" :disabled="loading">
                    {{ t('common.refresh') }}
                  </button>
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
                <div v-else class="cron-detail-grid">
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.schedule') }}</div>
                    <div class="cron-detail-value">{{ formatSchedule(selectedJob) }}</div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.payload') }}</div>
                    <div class="cron-detail-value pre">{{ payloadMessage }}</div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.nextRun') }}</div>
                    <div class="cron-detail-value">
                      {{ selectedJob.next_run_at_text || t('cron.status.noNext') }}
                    </div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.lastRun') }}</div>
                    <div class="cron-detail-value">
                      {{ selectedJob.last_run_at_text || t('common.none') }}
                    </div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.status') }}</div>
                    <div class="cron-detail-value">{{ selectedJob.last_status || t('common.unknown') }}</div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.error') }}</div>
                    <div class="cron-detail-value pre">
                      {{ selectedJob.last_error || t('common.none') }}
                    </div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.session') }}</div>
                    <div class="cron-detail-value">{{ selectedJob.session_id || t('common.none') }}</div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.agent') }}</div>
                    <div class="cron-detail-value">{{ selectedJob.agent_id || t('common.none') }}</div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.target') }}</div>
                    <div class="cron-detail-value">
                      {{ selectedJob.session_target || t('common.none') }}
                    </div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.enabled') }}</div>
                    <div class="cron-detail-value">
                      {{ selectedJob.enabled ? t('common.yes') : t('common.no') }}
                    </div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.deleteAfterRun') }}</div>
                    <div class="cron-detail-value">
                      {{ selectedJob.delete_after_run ? t('common.yes') : t('common.no') }}
                    </div>
                  </div>
                  <div class="cron-detail-item">
                    <div class="cron-detail-label">{{ t('cron.detail.dedupeKey') }}</div>
                    <div class="cron-detail-value">{{ selectedJob.dedupe_key || t('common.none') }}</div>
                  </div>
                </div>
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
                  <div v-else class="cron-run-list">
                    <div v-for="run in runs" :key="run.run_id" class="cron-run-item">
                      <div class="cron-run-main">
                        <div class="cron-run-status">
                          {{ formatRunStatus(run.status) }}
                        </div>
                        <div class="cron-run-time">{{ run.created_at_text || run.created_at }}</div>
                      </div>
                      <div class="cron-run-summary">
                        {{ run.summary || run.error || t('common.none') }}
                      </div>
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
import { computed, onMounted, ref } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  fetchCronJobs,
  fetchCronRuns,
  enableCronJob,
  disableCronJob,
  removeCronJob,
  runCronJob
} from '@/api/cron';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';

const { t } = useI18n();
const jobs = ref([]);
const runs = ref([]);
const loading = ref(false);
const runsLoading = ref(false);
const selectedJobId = ref('');

const selectedJob = computed(
  () => jobs.value.find((job) => job.job_id === selectedJobId.value) || null
);

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

const loadJobs = async () => {
  loading.value = true;
  try {
    const { data } = await fetchCronJobs();
    const items = data?.data?.jobs || [];
    jobs.value = Array.isArray(items) ? items : [];
    if (!jobs.value.length) {
      selectedJobId.value = '';
      runs.value = [];
      return;
    }
    if (!selectedJobId.value || !jobs.value.find((job) => job.job_id === selectedJobId.value)) {
      selectedJobId.value = jobs.value[0].job_id;
    }
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
  } finally {
    loading.value = false;
  }
};

const loadRuns = async (jobId) => {
  if (!jobId) {
    runs.value = [];
    return;
  }
  runsLoading.value = true;
  try {
    const { data } = await fetchCronRuns(jobId);
    const items = data?.data?.runs || [];
    runs.value = Array.isArray(items) ? items : [];
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
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
  await loadRuns(job.job_id);
};

const toggleEnable = async () => {
  const job = selectedJob.value;
  if (!job) {
    return;
  }
  try {
    const payload = { action: job.enabled ? 'disable' : 'enable', job: { job_id: job.job_id } };
    if (job.enabled) {
      await disableCronJob(payload);
    } else {
      await enableCronJob(payload);
    }
    await refreshAll();
  } catch (error) {
    ElMessage.error(resolveErrorMessage(error));
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
    await runCronJob({ action: 'run', job: { job_id: job.job_id } });
    await refreshAll();
  } catch (error) {
    if (error === 'cancel' || error === 'close') {
      return;
    }
    ElMessage.error(resolveErrorMessage(error));
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
    await removeCronJob({ action: 'remove', job: { job_id: job.job_id } });
    await refreshAll();
  } catch (error) {
    if (error === 'cancel' || error === 'close') {
      return;
    }
    ElMessage.error(resolveErrorMessage(error));
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
  await refreshAll();
});
</script>
