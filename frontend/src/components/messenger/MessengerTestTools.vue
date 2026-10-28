<template>
  <div class="messenger-test-tools" data-testid="messenger-test-tools">
    <div class="messenger-settings-row">
      <div class="messenger-settings-page-row-main">
        <i class="fa-solid fa-bug messenger-settings-page-row-icon" aria-hidden="true"></i>
        <div>
          <div class="messenger-settings-label">{{ t('messenger.settingsPage.general.debugEnabled') }}</div>
          <div class="messenger-settings-hint">{{ t('messenger.settingsPage.general.debugEnabledHint') }}</div>
        </div>
      </div>
      <div class="messenger-settings-page-row-actions">
        <label class="messenger-settings-switch">
          <input
            v-model="debugEnabled"
            type="checkbox"
            role="switch"
            :aria-label="t('messenger.settingsPage.general.debugEnabled')"
            data-testid="settings-debug-enabled"
            @change="handleDebugEnabledChange"
          />
          <span aria-hidden="true"></span>
        </label>
      </div>
    </div>

    <div class="messenger-settings-row">
      <div class="messenger-settings-page-row-main">
        <i class="fa-solid fa-gauge-high messenger-settings-page-row-icon" aria-hidden="true"></i>
        <div>
          <div class="messenger-settings-label">{{ t('messenger.perf.title') }}</div>
          <div class="messenger-settings-hint">{{ t('messenger.perf.hint') }}</div>
        </div>
      </div>
      <div class="messenger-settings-page-row-actions">
        <span class="messenger-settings-page-value" role="status">
          {{ t(performanceState.running ? 'messenger.perf.running' : performanceState.hasReport ? 'messenger.perf.stopped' : 'messenger.perf.idle') }}
        </span>
        <button
          class="messenger-settings-action"
          type="button"
          data-testid="perf-start"
          :disabled="performanceState.running"
          @click="chatPerf.start()"
        >
          {{ t('messenger.perf.start') }}
        </button>
        <button
          class="messenger-settings-action ghost"
          type="button"
          data-testid="perf-stop"
          :disabled="!performanceState.running"
          @click="chatPerf.stop()"
        >
          {{ t('messenger.perf.stop') }}
        </button>
        <button
          class="messenger-settings-action ghost"
          type="button"
          data-testid="perf-download"
          :disabled="!performanceState.hasReport"
          @click="downloadPerformance"
        >
          <i class="fa-solid fa-download" aria-hidden="true"></i>
          {{ t('messenger.perf.download') }}
        </button>
      </div>
    </div>

    <div class="messenger-settings-row">
      <div class="messenger-settings-page-row-main">
        <i class="fa-solid fa-file-code messenger-settings-page-row-icon" aria-hidden="true"></i>
        <div>
          <div class="messenger-settings-label">{{ t('messenger.settingsPage.general.debugExport') }}</div>
          <div class="messenger-settings-hint">
            {{ t('messenger.settingsPage.general.debugExportHint', { count: debugEventCount }) }}
          </div>
        </div>
      </div>
      <div class="messenger-settings-page-row-actions">
        <button
          class="messenger-settings-action ghost"
          type="button"
          data-testid="settings-debug-download"
          :disabled="debugEventCount === 0 && !performanceState.hasReport"
          @click="downloadDebugReport"
        >
          <i class="fa-solid fa-download" aria-hidden="true"></i>
          {{ t('messenger.settingsPage.general.debugDownload') }}
        </button>
        <button
          class="messenger-settings-action ghost"
          type="button"
          data-testid="settings-debug-clear"
          :disabled="debugEventCount === 0"
          @click="clearDebugHistory"
        >
          {{ t('messenger.settingsPage.general.debugClear') }}
        </button>
      </div>
    </div>

    <div v-if="error" class="messenger-settings-hint" role="alert">{{ error }}</div>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, shallowRef } from 'vue';

import { useI18n } from '@/i18n';
import { chatPerf } from '@/utils/chatPerf';
import {
  clearChatDebugHistory,
  getChatDebugStatus,
  isChatDebugEnabled,
  setChatDebugEnabled,
  setChatDebugVerboseEnabled,
  subscribeChatDebug
} from '@/utils/chatDebug';
import { downloadChatDebugReport } from '@/utils/chatDebugReport';

const { t } = useI18n();
const debugEnabled = ref(isChatDebugEnabled());
const performanceState = shallowRef(chatPerf.status());
const debugEventCount = ref(getChatDebugStatus().historyCount);
const error = ref('');
let refreshTimer: number | null = null;

const refreshState = () => {
  // Poll only scalar status while the panel is visible; never copy the event history here.
  debugEventCount.value = getChatDebugStatus().historyCount;
  debugEnabled.value = isChatDebugEnabled();
};

const handleDebugEnabledChange = () => {
  if (debugEnabled.value) {
    setChatDebugVerboseEnabled(true);
    chatPerf.start();
  } else {
    setChatDebugEnabled(false);
    chatPerf.stop();
  }
  refreshState();
};

const downloadPerformance = () => {
  error.value = '';
  try {
    if (!chatPerf.download()) {
      error.value = t('messenger.perf.downloadFailed');
    }
  } catch {
    error.value = t('messenger.perf.downloadFailed');
  }
};

const downloadDebugReport = () => {
  error.value = '';
  try {
    downloadChatDebugReport();
  } catch {
    error.value = t('messenger.settingsPage.general.debugDownloadFailed');
  }
};

const clearDebugHistory = () => {
  clearChatDebugHistory();
  refreshState();
};

const unsubscribePerformance = chatPerf.subscribe(() => { performanceState.value = chatPerf.status(); });
const unsubscribeDebug = subscribeChatDebug(refreshState);

onMounted(() => {
  refreshTimer = window.setInterval(refreshState, 1000);
});

onBeforeUnmount(() => {
  unsubscribePerformance();
  unsubscribeDebug();
  if (refreshTimer !== null) window.clearInterval(refreshTimer);
});
</script>

<style scoped>
.messenger-test-tools .messenger-settings-row {
  flex-wrap: wrap;
}
.messenger-test-tools .messenger-settings-page-row-main {
  flex: 1 1 240px;
}
.messenger-test-tools .messenger-settings-page-row-actions {
  margin-left: auto;
}
.messenger-test-tools input:focus-visible + span {
  outline: 2px solid var(--ui-accent-ink);
  outline-offset: 3px;
}
</style>
