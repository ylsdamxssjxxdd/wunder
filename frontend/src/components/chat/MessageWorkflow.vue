<template>
  <details v-if="visible" class="message-workflow" open>
    <summary>
      <span class="workflow-title">工作流</span>
      <span v-if="count" class="workflow-count">{{ count }}</span>
      <span v-if="loading" class="workflow-loading"><span class="spinner" /></span>
      <span class="workflow-spacer" />
      <button
        class="workflow-plan-btn"
        type="button"
        :class="{ 'is-active': planDialogVisible || hasPlan }"
        title="计划看板"
        aria-label="计划看板"
        @click.stop="openPlanDialog"
        @keydown.enter.stop.prevent="openPlanDialog"
        @keydown.space.stop.prevent="openPlanDialog"
      >
        <svg class="workflow-plan-icon" viewBox="0 0 24 24" aria-hidden="true">
          <rect x="3" y="3" width="7" height="7" rx="1" />
          <rect x="14" y="3" width="7" height="7" rx="1" />
          <rect x="3" y="14" width="7" height="7" rx="1" />
          <rect x="14" y="14" width="7" height="7" rx="1" />
        </svg>
      </button>
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
  <el-dialog
    v-model="planDialogVisible"
    title="计划看板"
    width="520px"
    class="plan-dialog"
    append-to-body
  >
    <div v-if="hasPlan" class="plan-board">
      <div v-if="planExplanation" class="plan-explanation">{{ planExplanation }}</div>
      <div class="plan-steps">
        <div
          v-for="(item, index) in plan.steps"
          :key="`${index}-${item.step}`"
          :class="['plan-step', `plan-step--${item.status}`]"
        >
          <span class="plan-index">{{ index + 1 }}</span>
          <div class="plan-text">{{ item.step }}</div>
          <span class="plan-status">{{ formatPlanStatus(item.status) }}</span>
        </div>
      </div>
    </div>
    <div v-else class="plan-empty">暂无计划</div>
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
  },
  plan: {
    type: Object,
    default: null
  },
  planVisible: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['update:planVisible']);

// 统计数量用于摘要展示
const count = computed(() => props.items.length);

const dialogVisible = ref(false);
const activeItem = ref(null);
const planDialogVisible = computed({
  get: () => props.planVisible,
  set: (value) => emit('update:planVisible', value)
});
const planExplanation = computed(() => String(props.plan?.explanation || '').trim());
const hasPlan = computed(
  () => Array.isArray(props.plan?.steps) && props.plan.steps.length > 0
);

const PLAN_STATUS_LABELS = {
  pending: '待办',
  in_progress: '进行中',
  completed: '完成'
};

const openDetail = (item) => {
  activeItem.value = item || null;
  dialogVisible.value = true;
};

const openPlanDialog = () => {
  planDialogVisible.value = true;
};

const getItemClasses = (item) => {
  if (!item?.isTool) return [];
  const category = item.toolCategory || 'default';
  return ['workflow-item--tool', `workflow-item--tool-${category}`];
};

const formatPlanStatus = (status) =>
  PLAN_STATUS_LABELS[status] || PLAN_STATUS_LABELS.pending;

const dialogTitle = computed(() => activeItem.value?.title || '节点详情');
const dialogDetail = computed(() => activeItem.value?.detail || '暂无详情');
</script>
