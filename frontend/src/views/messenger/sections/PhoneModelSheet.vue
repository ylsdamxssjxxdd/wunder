<template>
  <!-- 手机顶部模型胶囊的底部面板（对齐安卓版 model sheet）：
       模型目录复用 composerModelCatalog，切换仍走 controller 的同一套写回逻辑。 -->
  <div v-if="visible" class="phone-model-sheet" role="presentation" @click.self="close">
    <section
      class="phone-model-sheet-card"
      role="dialog"
      aria-modal="true"
      :aria-label="t('chat.composer.modelSheetTitle')"
    >
      <header class="phone-model-sheet-head">
        <span class="phone-model-sheet-title">{{ t('chat.composer.modelSheetTitle') }}</span>
        <button
          class="phone-model-sheet-close"
          type="button"
          :title="t('common.close')"
          :aria-label="t('common.close')"
          @click="close"
        >
          <i class="fa-solid fa-xmark" aria-hidden="true"></i>
        </button>
      </header>
      <ModelPickerPopover
        variant="sheet"
        :items="modelOptions"
        :active-model-id="modelName"
        :context-usage="contextUsage"
        :loading="modelCatalog.loading"
        :failed="modelCatalog.failed"
        :busy="switching"
        @select="handleSelect"
        @retry="reloadCatalog"
      />
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import ModelPickerPopover from '@/components/chat/ModelPickerPopover.vue';
import {
  composerModelCatalog,
  ensureComposerModelCatalog,
  type ComposerModelOption
} from '@/components/chat/composerModelCatalog';
import { useI18n } from '@/i18n';

type ContextUsageProp = {
  ratio: number | null;
  percentText: string;
  counts: string;
  level: '' | 'is-warning' | 'is-danger';
  overflow: boolean;
  compacted: boolean;
};

const props = defineProps({
  visible: {
    type: Boolean,
    default: false
  },
  modelName: {
    type: String,
    default: ''
  },
  contextUsage: {
    type: Object as () => ContextUsageProp,
    required: true
  },
  applyModel: {
    type: Function,
    default: null
  },
  reasoningEffort: {
    type: String,
    default: 'default'
  }
});

const emit = defineEmits(['update:visible']);

const { t } = useI18n();
const switching = ref(false);
const modelCatalog = computed(() => composerModelCatalog.value);
const modelOptions = computed<ComposerModelOption[]>(() =>
  Array.isArray(composerModelCatalog.value.items) ? composerModelCatalog.value.items : []
);

const close = () => {
  emit('update:visible', false);
};

const reloadCatalog = () => {
  void ensureComposerModelCatalog({ force: true });
};

watch(
  () => props.visible,
  (visible) => {
    if (visible) void ensureComposerModelCatalog();
  },
  { immediate: true }
);

const handleSelect = async (modelId: string) => {
  const target = String(modelId || '').trim();
  if (!target || switching.value) return;
  if (target === props.modelName) {
    close();
    return;
  }
  const applyModel = props.applyModel as
    | ((payload: { modelId: string; reasoningEffort: string }) => Promise<{
        ok?: boolean;
        message?: string;
      }>)
    | null;
  if (typeof applyModel !== 'function') {
    ElMessage.warning(t('chat.composer.modelUnavailable'));
    return;
  }
  switching.value = true;
  try {
    const result = await applyModel({ modelId: target, reasoningEffort: props.reasoningEffort });
    if (result?.ok === false) {
      ElMessage.error(String(result?.message || t('chat.composer.modelSwitchFailed')));
      return;
    }
    close();
    ElMessage.success(String(result?.message || t('chat.composer.modelSwitched', { name: target })));
  } catch (error) {
    ElMessage.error(String((error as Error)?.message || t('chat.composer.modelSwitchFailed')));
  } finally {
    switching.value = false;
  }
};
</script>

<style scoped>
.phone-model-sheet {
  position: fixed;
  inset: 0;
  z-index: 2100;
  display: flex;
  align-items: flex-end;
  justify-content: center;
  background: rgba(20, 22, 26, 0.42);
  overscroll-behavior: contain;
}

.phone-model-sheet-card {
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  width: 100%;
  max-height: min(72vh, 560px);
  padding: 8px 12px calc(12px + env(safe-area-inset-bottom, 0px));
  border-radius: 18px 18px 0 0;
  background: var(--mz-surface, #ffffff);
  box-shadow: 0 -12px 32px rgba(31, 35, 41, 0.18);
  animation: phone-model-sheet-in 180ms ease-out;
}

.phone-model-sheet-head {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 8px;
  height: 44px;
  padding: 0 2px 2px 4px;
}

.phone-model-sheet-title {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  color: var(--mz-text, #1f2329);
  font-size: 15px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.phone-model-sheet-close {
  display: inline-flex;
  flex-shrink: 0;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 0;
  border-radius: 9px;
  background: transparent;
  color: var(--mz-text-muted, #8a8f99);
  font-size: 14px;
  cursor: pointer;
}

.phone-model-sheet-close:active {
  background: var(--mz-hover, #f1efec);
}

@keyframes phone-model-sheet-in {
  from {
    transform: translateY(16px);
    opacity: 0;
  }
  to {
    transform: translateY(0);
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .phone-model-sheet-card {
    animation: none;
  }
}
</style>
