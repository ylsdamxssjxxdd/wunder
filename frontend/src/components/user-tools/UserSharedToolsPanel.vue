<template>
  <div class="user-tools-pane shared-tools-pane">
    <div class="list-header">
      <label>{{ t('userTools.shared.title') }}</label>
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
        <div v-if="statusMessage" class="user-tools-status list-status">{{ statusMessage }}</div>
      </div>
    </div>
    <div class="tips">{{ t('userTools.shared.tip') }}</div>
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
</template>

<script setup lang="ts">
import { onBeforeUnmount, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserToolsCatalog, saveUserSharedTools } from '@/api/userTools';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

const sharedTools = ref([]);
const selectedSet = ref(new Set());
const loading = ref(false);
const statusMessage = ref('');
const loadVersion = ref(0);
const saveTimer = ref(null);
const saveVersion = ref(0);
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

const updateStatus = (message: string = '') => {
  if (message) {
    statusMessage.value = message;
    return;
  }
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

const persistSelection = () => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
  saveTimer.value = setTimeout(async () => {
    saveTimer.value = null;
    const currentVersion = ++saveVersion.value;
    try {
      const payload = Array.from(selectedSet.value);
      await saveUserSharedTools({ shared_tools: payload });
      if (currentVersion === saveVersion.value) {
        updateStatus(t('userTools.shared.saved'));
        setTimeout(() => updateStatus(), 1200);
      }
    } catch (error) {
      if (currentVersion !== saveVersion.value) return;
      updateStatus(t('userTools.shared.saveFailed'));
      showApiError(error, t('userTools.shared.saveFailed'));
    }
  }, 400);
};

const toggleSelection = (name, checked) => {
  const next = new Set(selectedSet.value);
  if (checked) {
    next.add(name);
  } else {
    next.delete(name);
  }
  selectedSet.value = next;
  updateStatus();
  persistSelection();
};

const selectAll = () => {
  selectedSet.value = new Set(sharedTools.value.map((tool) => tool.name));
  updateStatus();
  persistSelection();
};

const clearAll = () => {
  selectedSet.value = new Set();
  updateStatus();
  persistSelection();
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
    selectedSet.value = new Set(selected.filter((name) => available.has(name)));
    updateStatus();
  } catch (error) {
    if (currentVersion !== loadVersion.value) {
      return;
    }
    updateStatus(t('userTools.shared.loadFailed'));
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

loadSharedTools();

onBeforeUnmount(() => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
});
</script>