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
        <div class="user-tools-title">共享工具</div>
        <button class="icon-btn" type="button" @click="close">×</button>
      </div>
    </template>

    <div class="user-tools-pane">
      <div class="list-header">
        <label>工具列表</label>
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
      <div class="tips">来自其他用户共享的工具需要勾选后才会注入系统提示词。</div>
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
    </div>

    <div class="user-tools-status">{{ statusMessage }}</div>

    <template #footer>
      <el-button class="user-tools-footer-btn" @click="close">关闭</el-button>
    </template>
  </el-dialog>
</template>

<script setup>
import { computed, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserToolsSummary } from '@/api/userTools';
import { useAuthStore } from '@/stores/auth';
import { loadSharedToolSelection, saveSharedToolSelection } from '@/utils/toolSelection';

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

const authStore = useAuthStore();
const userId = computed(() => authStore.user?.id || '');

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

const updateStatus = () => {
  if (!sharedTools.value.length) {
    statusMessage.value = '暂无共享工具';
    return;
  }
  statusMessage.value = `已选 ${selectedSet.value.size} / ${sharedTools.value.length}`;
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
  saveSharedToolSelection(userId.value, next);
  updateStatus();
};

const selectAll = () => {
  const next = new Set(sharedTools.value.map((tool) => tool.name));
  selectedSet.value = next;
  saveSharedToolSelection(userId.value, next);
  updateStatus();
};

const clearAll = () => {
  selectedSet.value = new Set();
  saveSharedToolSelection(userId.value, selectedSet.value);
  updateStatus();
};

const loadSharedTools = async () => {
  if (loading.value) return;
  loading.value = true;
  statusMessage.value = '正在加载...';
  const currentVersion = ++loadVersion.value;
  try {
    const { data } = await fetchUserToolsSummary();
    if (currentVersion !== loadVersion.value) {
      return;
    }
    const payload = data?.data || {};
    const list = Array.isArray(payload.shared_tools) ? payload.shared_tools : [];
    sharedTools.value = list.map(normalizeTool).filter(Boolean);
    const available = new Set(sharedTools.value.map((tool) => tool.name));
    const cached = loadSharedToolSelection(userId.value);
    const next = new Set([...cached].filter((name) => available.has(name)));
    selectedSet.value = next;
    saveSharedToolSelection(userId.value, next);
    updateStatus();
  } catch (error) {
    if (currentVersion !== loadVersion.value) {
      return;
    }
    statusMessage.value = `加载失败：${error.message || '请求失败'}`;
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
