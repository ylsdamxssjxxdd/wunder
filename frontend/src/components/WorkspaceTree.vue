<template>
  <div class="workspace-tree">
    <div class="workspace-header">目录</div>
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
const props = defineProps({
  treeData: {
    type: Array,
    default: () => []
  }
});

const emit = defineEmits(['select']);

const treeProps = {
  label: 'name',
  children: 'children'
};

const onNodeClick = (node) => {
  emit('select', node.id);
};
</script>
