<template>
  <div class="file-list">
    <div class="file-header">
      <span>{{ t('fileList.title') }}</span>
      <el-upload :show-file-list="false" :before-upload="handleUpload">
        <el-button type="primary" size="small">{{ t('common.upload') }}</el-button>
      </el-upload>
    </div>
    <el-table :data="files" height="420" @row-click="handleSelect">
      <el-table-column prop="name" :label="t('fileList.column.name')" />
      <el-table-column prop="size" :label="t('fileList.column.size')" width="100" />
      <el-table-column :label="t('fileList.column.type')" width="160">
        <template #default="scope">
          {{ scope.row.extension || '-' }}
        </template>
      </el-table-column>
      <el-table-column :label="t('fileList.column.action')" width="160">
        <template #default="scope">
          <el-button type="primary" link @click="$emit('download', scope.row)">
            {{ t('common.download') }}
          </el-button>
          <el-button type="danger" link @click="$emit('delete', scope.row)">
            {{ t('common.delete') }}
          </el-button>
        </template>
      </el-table-column>
    </el-table>
  </div>
</template>

<script setup>
import { useI18n } from '@/i18n';

defineProps({
  files: {
    type: Array,
    default: () => []
  }
});

const { t } = useI18n();
const emit = defineEmits(['upload', 'download', 'delete', 'select']);

const handleUpload = (file) => {
  emit('upload', file);
  return false;
};

const handleSelect = (row) => {
  emit('select', row);
};
</script>
