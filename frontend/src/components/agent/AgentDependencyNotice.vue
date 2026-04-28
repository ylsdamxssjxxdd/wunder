<template>
  <div v-if="hasMissing" class="agent-dependency-notice">
    <div v-if="isHidden" class="agent-dependency-notice-hidden">
      <span>{{ t('portal.agent.dependencies.hiddenHint') }}</span>
      <button class="agent-dependency-notice-toggle" type="button" @click="showNotice">
        {{ t('portal.agent.dependencies.show') }}
      </button>
    </div>
    <el-alert v-else type="warning" :closable="allowHide" :close-text="closeText" show-icon @close="hideNotice">
      <template #title>
        {{ t('portal.agent.dependencies.title') }}
      </template>
      <div class="agent-dependency-notice-body">
        <div v-if="missingToolNames.length" class="agent-dependency-notice-group">
          <div class="agent-dependency-notice-label">{{ t('portal.agent.dependencies.missingTools') }}</div>
          <div class="agent-dependency-notice-tags">
            <el-tag v-for="name in missingToolNames" :key="`missing-tool-${name}`" size="small" type="warning">
              {{ name }}
            </el-tag>
          </div>
        </div>
        <div v-if="missingSkillNames.length" class="agent-dependency-notice-group">
          <div class="agent-dependency-notice-label">{{ t('portal.agent.dependencies.missingSkills') }}</div>
          <div class="agent-dependency-notice-tags">
            <el-tag v-for="name in missingSkillNames" :key="`missing-skill-${name}`" size="small" type="danger">
              {{ name }}
            </el-tag>
          </div>
        </div>
        <div class="agent-dependency-notice-hint">
          {{ t('portal.agent.dependencies.hint') }}
        </div>
        <div v-if="allowIgnore" class="agent-dependency-notice-actions">
          <el-button size="small" text type="primary" @click="ignoreNotice">
            {{ t('portal.agent.dependencies.ignore') }}
          </el-button>
        </div>
      </div>
    </el-alert>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

const HIDDEN_NOTICE_STORAGE_PREFIX = 'wunder.agent.dependency.notice.hidden';

function readHiddenFingerprint(storageKey: string): string {
  if (!storageKey || typeof window === 'undefined') return '';
  try {
    return String(window.localStorage.getItem(storageKey) || '');
  } catch {
    return '';
  }
}

function writeHiddenFingerprint(storageKey: string, value: string): void {
  if (!storageKey || typeof window === 'undefined') return;
  try {
    if (value) {
      window.localStorage.setItem(storageKey, value);
    } else {
      window.localStorage.removeItem(storageKey);
    }
  } catch {
    return;
  }
}

const props = defineProps({
  missingToolNames: {
    type: Array as () => string[],
    default: () => []
  },
  missingSkillNames: {
    type: Array as () => string[],
    default: () => []
  },
  noticeKey: {
    type: String,
    default: ''
  },
  allowHide: {
    type: Boolean,
    default: true
  },
  allowIgnore: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits<{
  ignore: [];
}>();

const { t } = useI18n();
const hasMissing = computed(() => props.missingToolNames.length > 0 || props.missingSkillNames.length > 0);
const hiddenFingerprint = ref('');

const dependencyFingerprint = computed(() => {
  const toolNames = [...props.missingToolNames].map((name) => String(name || '').trim()).filter(Boolean).sort();
  const skillNames = [...props.missingSkillNames].map((name) => String(name || '').trim()).filter(Boolean).sort();
  return JSON.stringify({ toolNames, skillNames });
});

const storageKey = computed(() => {
  const noticeKey = String(props.noticeKey || '').trim();
  if (!noticeKey) return '';
  return `${HIDDEN_NOTICE_STORAGE_PREFIX}:${noticeKey}`;
});

const closeText = computed(() => (props.allowHide ? t('portal.agent.dependencies.hide') : ''));
const isHidden = computed(
  () => hasMissing.value && Boolean(dependencyFingerprint.value) && hiddenFingerprint.value === dependencyFingerprint.value
);

function syncHiddenState(): void {
  const nextFingerprint = readHiddenFingerprint(storageKey.value);
  hiddenFingerprint.value = nextFingerprint;
}

function hideNotice(): void {
  if (!props.allowHide || !hasMissing.value) return;
  hiddenFingerprint.value = dependencyFingerprint.value;
  writeHiddenFingerprint(storageKey.value, hiddenFingerprint.value);
}

function showNotice(): void {
  hiddenFingerprint.value = '';
  writeHiddenFingerprint(storageKey.value, '');
}

function ignoreNotice(): void {
  emit('ignore');
}

watch(storageKey, syncHiddenState, { immediate: true });
</script>

<style scoped>
.agent-dependency-notice {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.agent-dependency-notice-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.agent-dependency-notice-hidden {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 12px;
  border: 1px dashed var(--el-border-color);
  border-radius: 10px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.agent-dependency-notice-toggle {
  border: none;
  background: transparent;
  color: var(--el-color-primary);
  cursor: pointer;
  font-size: 12px;
  padding: 0;
}

.agent-dependency-notice-toggle:hover {
  color: var(--el-color-primary-light-3);
}

.agent-dependency-notice-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.agent-dependency-notice-label {
  font-size: 12px;
  font-weight: 600;
}

.agent-dependency-notice-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.agent-dependency-notice-hint {
  font-size: 12px;
  line-height: 1.5;
  opacity: 0.85;
}

.agent-dependency-notice-actions {
  display: flex;
  justify-content: flex-end;
}
</style>
