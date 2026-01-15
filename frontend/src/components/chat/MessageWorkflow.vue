<template>
  <details v-if="visible" class="message-workflow" open>
    <summary>
      工作流
      <span v-if="count" class="workflow-count">{{ count }}</span>
      <span v-if="loading" class="workflow-loading"><span class="spinner" /></span>
    </summary>
    <div class="workflow-content">
      <div v-if="items.length === 0" class="workflow-empty">等待事件...</div>
      <div
        v-for="item in items"
        :key="item.id"
        :class="['workflow-item', ...getItemClasses(item)]"
        role="button"
        tabindex="0"
        @click="openDetail(item)"
        @keydown.enter.prevent="openDetail(item)"
      >
        <span :class="['status-indicator', item.status]" />
        <div class="workflow-text">
          <div class="workflow-title">{{ item.title }}</div>
        </div>
      </div>
    </div>
  </details>
  <el-dialog
    v-model="dialogVisible"
    title="节点详情"
    width="560px"
    class="workflow-dialog"
    append-to-body
  >
    <div class="workflow-dialog-title">{{ dialogTitle }}</div>
    <pre class="workflow-dialog-detail">{{ dialogDetail }}</pre>
  </el-dialog>
</template>

<script setup>
import { computed, ref } from 'vue';

// 工作流事件展示组件：承载 SSE 事件列表
const props = defineProps({
  items: {
    type: Array,
    default: () => []
  },
  loading: {
    type: Boolean,
    default: false
  },
  visible: {
    type: Boolean,
    default: false
  }
});

// 统计数量用于摘要展示
const count = computed(() => props.items.length);

const dialogVisible = ref(false);
const activeItem = ref(null);

const openDetail = (item) => {
  activeItem.value = item || null;
  dialogVisible.value = true;
};

const getItemClasses = (item) => {
  if (!item?.isTool) return [];
  const category = item.toolCategory || 'default';
  return ['workflow-item--tool', `workflow-item--tool-${category}`];
};

const dialogTitle = computed(() => activeItem.value?.title || '节点详情');
const dialogDetail = computed(() => activeItem.value?.detail || '暂无详情');
</script>
