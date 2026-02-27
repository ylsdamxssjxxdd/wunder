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
  --fw-text: #202020;
  --fw-muted: #808080;
  --fw-bg: #ffffff;
  --fw-shadow: 0 18px 42px rgba(15, 23, 42, 0.16);
  --fw-border: #dfe3ea;
  --fw-divider: #ececec;
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
  border: 1px solid #d7d7d7;
  border-radius: 10px;
  background: #f7f7f7;
  color: inherit;
  cursor: pointer;
  transition: border-color 0.2s ease, background 0.2s ease, color 0.2s ease;
}

.feature-window-close:hover {
  border-color: rgba(var(--ui-accent-rgb), 0.45);
  color: var(--ui-accent);
}

.feature-window-body {
  min-height: 0;
  flex: 1;
  overflow: hidden;
}
</style>
