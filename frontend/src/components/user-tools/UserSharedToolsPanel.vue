<template>
  <div class="user-tools-pane shared-tools-pane">
    <div class="list-header">
      <label>共享工具</label>
      <div class="header-actions">
        <button
          class="user-tools-btn secondary compact"
          type="button"
          :disabled="loading || !sharedTools.length"
          @click="selectAll"
        >
          全选
        </button>
        <button
          class="user-tools-btn secondary compact"
          type="button"
          :disabled="loading || !sharedTools.length"
          @click="clearAll"
        >
          清空
        </button>
        <button class="user-tools-btn secondary compact" type="button" :disabled="loading" @click="reload">
          刷新
        </button>
      </div>
    </div>
    <div class="tips">勾选后共享工具才会进入你的工具池，可挂载给智能体或会话。</div>
    <div class="tool-list">
      <div v-if="loading" class="empty-text">加载中...</div>
      <div v-else-if="!sharedTools.length" class="empty-text">暂无共享工具</div>
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
            @change="toggleSelection(tool.name, $event.target.checked)"
          />
          <span>使用</span>
        </label>
        <label class="tool-item-info">
          <strong>{{ tool.name }}</strong>
          <span class="muted">{{ buildToolDesc(tool) }}</span>
        </label>
      </div>
    </div>

    <div class="user-tools-status">{{ statusMessage }}</div>
  </div>
</template>

<script setup>
import { onBeforeUnmount, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserToolsCatalog, saveUserSharedTools } from '@/api/userTools';

const sharedTools = ref([]);
const selectedSet = ref(new Set());
const loading = ref(false);
const statusMessage = ref('');
const loadVersion = ref(0);
const saveTimer = ref(null);
const saveVersion = ref(0);

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
    parts.push(`来自 ${tool.ownerId}`);
  }
  return parts.join(' · ') || '暂无描述';
};

const updateStatus = (message) => {
  if (message) {
    statusMessage.value = message;
    return;
  }
  if (!sharedTools.value.length) {
    statusMessage.value = '暂无共享工具';
    return;
  }
  statusMessage.value = `已选 ${selectedSet.value.size} / ${sharedTools.value.length}`;
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
        updateStatus('已保存');
        setTimeout(() => updateStatus(), 1200);
      }
    } catch (error) {
      if (currentVersion !== saveVersion.value) return;
      updateStatus('保存失败，请稍后重试');
      ElMessage.error(error.response?.data?.detail || '共享工具保存失败');
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
  statusMessage.value = '正在加载...';
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
    updateStatus('加载失败，请稍后重试');
    ElMessage.error(error.response?.data?.detail || '共享工具加载失败');
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
