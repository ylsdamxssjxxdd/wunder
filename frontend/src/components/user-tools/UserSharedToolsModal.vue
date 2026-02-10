<template>
  <el-dialog
    v-model="visibleProxy"
    class="user-tools-dialog user-tools-subdialog user-tools-quick"
    width="760px"
    top="8vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="user-tools-header">
        <div class="user-tools-title">{{ t('userTools.shared.title') }}</div>
        <button class="icon-btn" type="button" @click="close">×</button>
      </div>
    </template>

    <div class="user-tools-pane">
      <div class="list-header">
        <label>{{ t('userTools.shared.list.title') }}</label>
        <div class="header-actions">
          <button
            class="user-tools-btn secondary compact"
            type="button"
            :disabled="loading || !sharedTools.length"
            @click="selectAll"
          >
            {{ t('common.selectAll') }}
          </button>
          <button
            class="user-tools-btn secondary compact"
            type="button"
            :disabled="loading || !sharedTools.length"
            @click="clearAll"
          >
            {{ t('common.clear') }}
          </button>
          <button class="user-tools-btn secondary compact" type="button" :disabled="loading" @click="reload">
            {{ t('common.refresh') }}
          </button>
        </div>
      </div>
      <div class="tips">{{ t('userTools.shared.modal.tip') }}</div>
      <div class="tool-list">
        <div v-if="loading" class="empty-text">{{ t('common.loading') }}</div>
        <div v-else-if="!sharedTools.length" class="empty-text">{{ t('userTools.shared.empty') }}</div>
        <div
          v-else
          v-for="tool in sharedTools"
          :key="tool.name"
          class="tool-item tool-item-single"
          @click="toggleSelection(tool.name, !isSelected(tool.name))"
        >
          <label class="tool-check" @click.stop>
            <input
              type="checkbox"
              :checked="isSelected(tool.name)"
              @change="toggleSelection(tool.name, ($event.target as HTMLInputElement).checked)"
            />
            <span>{{ t('common.use') }}</span>
          </label>
          <label class="tool-item-info">
            <strong>{{ tool.name }}</strong>
            <span class="muted">{{ buildToolDesc(tool) }}</span>
          </label>
        </div>
      </div>
    </div>

    <div class="user-tools-status">{{ statusMessage }}</div>

    <template #footer>
      <el-button class="user-tools-footer-btn" @click="close">{{ t('common.close') }}</el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserToolsCatalog, saveUserSharedTools } from '@/api/userTools';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

const props = defineProps({
  modelValue: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['update:modelValue']);

const visibleProxy = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value)
});

const sharedTools = ref([]);
const selectedSet = ref(new Set());
const loading = ref(false);
const statusMessage = ref('');
const loadVersion = ref(0);
const { t } = useI18n();

const normalizeTool = (tool) => {
  if (!tool) return null;
  if (typeof tool === 'string') {
    const name = tool.trim();
    return name ? { name, description: '', ownerId: '' } : null;
  }
  const name = String(tool.name || '').trim();
  if (!name) return null;
  return {
    name,
    description: tool.description || '',
    ownerId: tool.owner_id || tool.ownerId || ''
  };
};

const buildToolDesc = (tool) => {
  const parts = [];
  if (tool.description) {
    parts.push(tool.description);
  }
  if (tool.ownerId) {
    parts.push(t('userTools.shared.source', { owner: tool.ownerId }));
  }
  return parts.join(' · ') || t('common.noDescription');
};

const updateStatus = () => {
  if (!sharedTools.value.length) {
    statusMessage.value = t('userTools.shared.empty');
    return;
  }
  statusMessage.value = t('userTools.shared.selection', {
    selected: selectedSet.value.size,
    total: sharedTools.value.length
  });
};

const isSelected = (name) => selectedSet.value.has(name);

const toggleSelection = (name, checked) => {
  const next = new Set(selectedSet.value);
  if (checked) {
    next.add(name);
  } else {
    next.delete(name);
  }
  selectedSet.value = next;
  saveSharedToolSelection();
  updateStatus();
};

const selectAll = () => {
  const next = new Set(sharedTools.value.map((tool) => tool.name));
  selectedSet.value = next;
  saveSharedToolSelection();
  updateStatus();
};

const clearAll = () => {
  selectedSet.value = new Set();
  saveSharedToolSelection();
  updateStatus();
};

const saveSharedToolSelection = async () => {
  try {
    const payload = Array.from(selectedSet.value);
    await saveUserSharedTools({ shared_tools: payload });
  } catch (error) {
    showApiError(error, t('userTools.shared.saveFailed'));
  }
};

const loadSharedTools = async () => {
  if (loading.value) return;
  loading.value = true;
  statusMessage.value = t('common.loading');
  const currentVersion = ++loadVersion.value;
  try {
    const { data } = await fetchUserToolsCatalog();
    if (currentVersion !== loadVersion.value) {
      return;
    }
    const payload = data?.data || {};
    const list = Array.isArray(payload.shared_tools) ? payload.shared_tools : [];
    sharedTools.value = list.map(normalizeTool).filter(Boolean);
    const available = new Set(sharedTools.value.map((tool) => tool.name));
    const selected = Array.isArray(payload.shared_tools_selected)
      ? payload.shared_tools_selected.map((name) => String(name).trim())
      : [];
    const next = new Set(selected.filter((name) => available.has(name)));
    selectedSet.value = next;
    updateStatus();
  } catch (error) {
    if (currentVersion !== loadVersion.value) {
      return;
    }
    statusMessage.value = t('userTools.shared.loadFailedWithMessage', {
      message: error.message || t('common.requestFailed')
    });
    showApiError(error, t('userTools.shared.loadFailed'));
  } finally {
    if (currentVersion === loadVersion.value) {
      loading.value = false;
    }
  }
};

const reload = () => {
  loadSharedTools();
};

const close = () => {
  visibleProxy.value = false;
};

watch(
  () => props.modelValue,
  (value) => {
    if (value) {
      loadSharedTools();
    } else {
      statusMessage.value = '';
    }
  }
);
</script>