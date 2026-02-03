<template>
  <details v-if="visible" class="message-workflow">
    <summary>
      <span class="workflow-title">{{ t('chat.workflow.title') }}</span>
      <span v-if="count" class="workflow-count">{{ count }}</span>
      <span v-if="loading" class="workflow-loading"><span class="spinner" /></span>
      <span v-if="latestItem" class="workflow-latest" :title="latestTitle">{{ latestTitle }}</span>
      <span v-else class="workflow-spacer" />
      <button
        class="workflow-plan-btn"
        type="button"
        :class="{ 'is-active': planDialogVisible }"
        :title="t('chat.workflow.plan.title')"
        :aria-label="t('chat.workflow.plan.title')"
        @click.stop="openPlanDialog"
        @keydown.enter.stop.prevent="openPlanDialog"
        @keydown.space.stop.prevent="openPlanDialog"
      >
        <i class="fa-solid fa-table-cells-large workflow-plan-icon" aria-hidden="true"></i>
      </button>
    </summary>
    <div class="workflow-content">
      <div v-if="items.length === 0" class="workflow-empty">{{ t('chat.workflow.empty') }}</div>
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
          <div class="workflow-title">{{ formatWorkflowTitle(item.title) }}</div>
        </div>
      </div>
    </div>
  </details>
  <el-dialog
    v-model="dialogVisible"
    :title="t('chat.workflow.nodeDetailTitle')"
    width="560px"
    class="workflow-dialog"
    append-to-body
  >
    <div class="workflow-dialog-title">{{ dialogTitle }}</div>
    <pre class="workflow-dialog-detail">{{ dialogDetail }}</pre>
  </el-dialog>
  <el-dialog
    v-model="planDialogVisible"
    :title="t('chat.workflow.plan.title')"
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
    <div v-else class="plan-empty">{{ t('chat.workflow.plan.empty') }}</div>
  </el-dialog>
</template>

<script setup>
import { computed, ref } from 'vue';

import { useI18n } from '@/i18n';

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
const { t } = useI18n();

// 统计数量用于摘要展示
const count = computed(() => props.items.length);
const latestItem = computed(() =>
  props.items.length > 0 ? props.items[props.items.length - 1] : null
);

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

const formatPlanStatus = (status) => {
  if (status === 'completed') return t('chat.workflow.plan.status.completed');
  if (status === 'in_progress') return t('chat.workflow.plan.status.inProgress');
  return t('chat.workflow.plan.status.pending');
};

const dialogTitle = computed(() =>
  activeItem.value?.title
    ? formatWorkflowTitle(activeItem.value.title)
    : t('chat.workflow.nodeDetailTitle')
);
const dialogDetail = computed(() => activeItem.value?.detail || t('chat.workflow.nodeEmpty'));

const formatWorkflowTitle = (rawTitle) => {
  const title = String(rawTitle || '').trim();
  if (!title) return '';
  if (title === '模型输出') return t('chat.workflow.modelOutput');
  if (title === '最终回复') return t('chat.workflow.finalResponse');
  if (title === '问询面板') return t('chat.workflow.questionPanel');
  if (title === '计划更新') return t('chat.workflow.planUpdate');
  if (title === '模型请求体') return t('chat.workflow.modelRequest');
  if (title === '模型请求摘要') return t('chat.workflow.modelRequestSummary');
  if (title === '进度更新') return t('chat.workflow.progressUpdate');
  if (title === '错误') return t('chat.workflow.error');

  if (title.startsWith('调用工具：')) {
    const tool = title.replace('调用工具：', '').trim();
    return t('chat.workflow.toolCall', {
      tool: tool || t('chat.workflow.toolUnknown')
    });
  }
  if (title.startsWith('工具结果：')) {
    const tool = title.replace('工具结果：', '').trim();
    return t('chat.workflow.toolResult', {
      tool: tool || t('chat.workflow.toolUnknown')
    });
  }
  if (title.startsWith('工具输出：')) {
    const tool = title.replace('工具输出：', '').trim();
    return t('chat.workflow.toolOutput', {
      tool: tool || t('chat.workflow.toolUnknown')
    });
  }
  if (title === '工具输出') {
    return t('chat.workflow.toolOutput', { tool: t('chat.workflow.toolUnknown') });
  }
  if (title.startsWith('知识库请求体')) {
    const match = title.match(/^知识库请求体(?:（(.+)）)?$/);
    const base = match?.[1];
    return base
      ? t('chat.workflow.knowledgeRequestWithBase', { base })
      : t('chat.workflow.knowledgeRequest');
  }
  if (title.startsWith('阶段：')) {
    const stage = title.replace('阶段：', '').trim();
    return t('chat.workflow.stage', { stage });
  }
  const modelRoundMatch = title.match(/^调用模型（第\s*(\d+)\s*轮）$/);
  if (modelRoundMatch) {
    return t('chat.workflow.modelCallRound', { round: modelRoundMatch[1] });
  }
  if (title.startsWith('事件：')) {
    const event = title.replace('事件：', '').trim();
    return t('chat.workflow.event', { event });
  }
  return title;
};

const latestTitle = computed(() =>
  latestItem.value ? formatWorkflowTitle(latestItem.value.title) : ''
);
</script>
