<template>
  <div class="file-preview">
    <div class="preview-title">预览</div>
    <div v-if="file">
      <p>名称：{{ file.name }}</p>
      <p>路径：{{ file.path }}</p>
      <p>大小：{{ file.size }}</p>
      <p>类型：{{ file.extension || '-' }}</p>
      <p>更新时间：{{ file.updated_time || '-' }}</p>
    </div>
    <div v-else class="preview-empty">请选择文件查看详情</div>
  </div>
</template>

<script setup>
defineProps({
  file: {
    type: Object,
    default: null
  }
});
</script>
