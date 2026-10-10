<template>
  <div class="interlink-node-switcher">
    <el-select
      class="interlink-node-select"
      size="small"
      :model-value="modelValue"
      :loading="loading"
      :aria-label="t('interlink.switcher.label')"
      @update:model-value="handleSelect"
    >
      <el-option value="cloud" :label="cloudLabel">
        <span class="interlink-node-option">
          <i class="fa-solid fa-cloud interlink-node-option-icon" aria-hidden="true"></i>
          <span class="interlink-node-option-name">{{ cloudLabel }}</span>
          <span class="interlink-node-option-tag">{{ t('interlink.switcher.defaultTag') }}</span>
        </span>
      </el-option>
      <el-option
        v-for="node in deviceNodes"
        :key="node.node_id"
        :value="`device:${node.node_id}`"
        :label="describeNode(node)"
      >
        <span class="interlink-node-option">
          <span
            class="interlink-status-dot"
            :class="statusDotClass(node.status)"
            :title="t(statusLabelKey(node.status))"
            aria-hidden="true"
          ></span>
          <i :class="nodeTypeIcon(node.node_type)" class="interlink-node-option-icon" aria-hidden="true"></i>
          <span class="interlink-node-option-name">{{ node.label || node.node_id }}</span>
          <span class="interlink-node-option-tag">{{ t(nodeTypeLabelKey(node.node_type)) }}</span>
          <span class="interlink-node-option-status">{{ t(statusLabelKey(node.status)) }}</span>
        </span>
      </el-option>
    </el-select>
    <button
      class="interlink-node-refresh"
      type="button"
      :title="t('common.refresh')"
      :aria-label="t('common.refresh')"
      :disabled="loading"
      @click="emit('refresh')"
    >
      <i class="fa-solid fa-rotate-right" aria-hidden="true"></i>
    </button>
    <!-- 单根模板：宿主会传 class，多根模板拿不到属性透传。 -->
    <div v-if="!deviceNodes.length && !loading" class="interlink-node-switcher-hint">
      {{ t('interlink.switcher.noDevices') }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import type { InterlinkNode } from '@/api/interlink';
import { useI18n } from '@/i18n';
import {
  INTERLINK_NODE_MAX_RENDERED,
  isDeviceNode,
  nodeTypeIcon,
  nodeTypeLabelKey,
  statusDotClass,
  statusLabelKey
} from './interlinkNodeModel';

const props = defineProps<{
  /** 'cloud' | 'device:<id>'（§6.3 与 URL `?node=` 同一形态）。 */
  modelValue: string;
  nodes: InterlinkNode[];
  loading?: boolean;
}>();

const emit = defineEmits<{
  'update:modelValue': [value: string];
  refresh: [];
}>();

const { t } = useI18n();

// 下拉里最多渲染 60 条，超出部分靠节点分页继续累加，不一次铺开。
const deviceNodes = computed(() =>
  (props.nodes || []).filter(isDeviceNode).slice(0, INTERLINK_NODE_MAX_RENDERED)
);

const cloudLabel = computed(() => t('interlink.switcher.cloud'));

const describeNode = (node: InterlinkNode): string => {
  const name = node.label || node.node_id;
  return `${name} · ${t(nodeTypeLabelKey(node.node_type))} · ${t(statusLabelKey(node.status))}`;
};

const handleSelect = (value: unknown): void => {
  const next = String(value || 'cloud').trim() || 'cloud';
  if (next === props.modelValue) return;
  emit('update:modelValue', next);
};
</script>
