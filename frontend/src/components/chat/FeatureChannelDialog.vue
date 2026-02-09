<template>
  <el-dialog
    v-model="visible"
    class="feature-window-dialog feature-window-dialog--channel"
    width="1120px"
    top="6vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="feature-window-header">
        <div class="feature-window-title">{{ t('chat.features.channels') }}</div>
        <button class="feature-window-close" type="button" @click="visible = false">&times;</button>
      </div>
    </template>

    <div class="feature-window-body">
      <UserChannelSettingsPanel ref="panelRef" mode="dialog" :agent-id="agentId" @changed="handleChanged" />
    </div>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import UserChannelSettingsPanel from '@/components/channels/UserChannelSettingsPanel.vue';
import { useI18n } from '@/i18n';

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

const panelRef = ref(null);

const handleChanged = () => {};

watch(
  () => visible.value,
  (value) => {
    if (value) {
      panelRef.value?.refreshAll?.();
    }
  }
);
</script>

<style scoped>
:global(.feature-window-dialog--channel.el-dialog) {
  --fw-text: #e2e8f0;
  --fw-muted: #94a3b8;
  --fw-bg: linear-gradient(160deg, #070d1a, #0b1426);
  --fw-shadow: 0 24px 56px rgba(8, 12, 24, 0.55);
  --fw-border: rgba(51, 65, 85, 0.72);
  --fw-divider: rgba(51, 65, 85, 0.62);
  width: min(96vw, 1200px) !important;
  max-width: 1200px;
  height: min(86vh, 840px);
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

:global(:root[data-user-theme='light'] .feature-window-dialog--channel.el-dialog) {
  --fw-text: #0f172a;
  --fw-muted: #64748b;
  --fw-bg: linear-gradient(180deg, #ffffff, #f7faff);
  --fw-shadow: 0 18px 40px rgba(15, 23, 42, 0.16);
  --fw-border: rgba(148, 163, 184, 0.52);
  --fw-divider: rgba(148, 163, 184, 0.42);
  color-scheme: light;
}

:global(.feature-window-dialog--channel .el-dialog__header) {
  border-bottom: 1px solid var(--fw-divider);
  padding: 14px 18px;
}

:global(.feature-window-dialog--channel .el-dialog__body) {
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
  border: 1px solid rgba(148, 163, 184, 0.45);
  border-radius: 10px;
  background: rgba(15, 23, 42, 0.45);
  color: inherit;
  cursor: pointer;
}

.feature-window-close:hover {
  border-color: rgba(56, 189, 248, 0.7);
}

.feature-window-body {
  min-height: 0;
  flex: 1;
  overflow: hidden;
}
</style>
