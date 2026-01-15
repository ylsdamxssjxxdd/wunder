<template>
  <div class="file-list">
    <div class="file-header">
      <span>文件列表</span>
      <el-upload
        :show-file-list="false"
        :before-upload="handleUpload"
      >
        <el-button type="primary" size="small">上传文件</el-button>
      </el-upload>
    </div>
    <el-table :data="files" height="420" @row-click="handleSelect">
      <el-table-column prop="name" label="名称" />
      <el-table-column prop="size" label="大小" width="100" />
      <el-table-column label="类型" width="160">
        <template #default="scope">
          {{ scope.row.extension || '-' }}
        </template>
      </el-table-column>
      <el-table-column label="操作" width="160">
        <template #default="scope">
          <el-button type="primary" link @click="$emit('download', scope.row)">下载</el-button>
          <el-button type="danger" link @click="$emit('delete', scope.row)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>
  </div>
</template>

<script setup>
defineProps({
  files: {
    type: Array,
    default: () => []
  }
});

const emit = defineEmits(['upload', 'download', 'delete', 'select']);

const handleUpload = (file) => {
  emit('upload', file);
  return false;
};

const handleSelect = (row) => {
  emit('select', row);
};
</script>
