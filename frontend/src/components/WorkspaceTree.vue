<template>
  <div class="workspace-tree">
    <div class="workspace-header">{{ t('workspace.tree.title') }}</div>
    <el-tree
      :data="treeData"
      :props="treeProps"
      node-key="id"
      highlight-current
      @node-click="onNodeClick"
    />
  </div>
</template>

<script setup>
import { useI18n } from '@/i18n';

const props = defineProps({
  treeData: {
    type: Array,
    default: () => []
  }
});

const emit = defineEmits(['select']);
const { t } = useI18n();

const treeProps = {
  label: 'name',
  children: 'children'
};

const onNodeClick = (node) => {
  emit('select', node.id);
};
</script>
