<template>
  <section class="messenger-settings-card" data-testid="chat-performance-capture">
    <div class="messenger-settings-head">
      <div class="messenger-settings-title">{{ t('messenger.perf.title') }}</div>
      <span class="messenger-settings-hint" role="status">{{ t(state.running ? 'messenger.perf.running' : state.hasReport ? 'messenger.perf.stopped' : 'messenger.perf.idle') }}</span>
    </div>
    <div class="messenger-settings-hint">{{ t('messenger.perf.hint') }}</div>
    <div class="capture-actions">
      <button class="messenger-settings-action" type="button" data-testid="perf-start" :disabled="state.running" @click="chatPerf.start()">
        {{ t('messenger.perf.start') }}
      </button>
      <button class="messenger-settings-action" type="button" data-testid="perf-download" :disabled="!state.hasReport" @click="download">
        <i class="fa-solid fa-download" aria-hidden="true"></i>{{ t('messenger.perf.download') }}
      </button>
      <button class="messenger-settings-action ghost" type="button" data-testid="perf-stop" :disabled="!state.running" @click="chatPerf.stop()">
        {{ t('messenger.perf.stop') }}
      </button>
    </div>
    <div v-if="error" class="messenger-settings-hint" role="alert">{{ error }}</div>
  </section>
</template>

<script setup lang="ts">
import { onBeforeUnmount, shallowRef } from 'vue';
import { useI18n } from '@/i18n';
import { chatPerf } from '@/utils/chatPerf';

const { t } = useI18n();
const state = shallowRef(chatPerf.status());
const error = shallowRef('');
const unsubscribe = chatPerf.subscribe(() => { state.value = chatPerf.status(); });
onBeforeUnmount(unsubscribe);
const download = () => {
  error.value = '';
  try { chatPerf.download(); } catch { error.value = t('messenger.perf.downloadFailed'); }
};
</script>

<style scoped>
.capture-actions { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 12px; }
.capture-actions button { min-height: 34px; }
.capture-actions button:disabled { opacity: 0.5; cursor: default; }
.capture-actions button:focus-visible { outline: 2px solid var(--ui-accent); outline-offset: 2px; }
</style>
