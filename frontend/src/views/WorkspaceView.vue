<template>
  <div class="workspace-view">
    <div class="workspace-sidebar">
      <div class="workspace-actions">
        <el-button type="primary" size="small" @click="createFolder">新建文件夹</el-button>
      </div>
      <WorkspaceTree :tree-data="workspaceStore.folders" @select="handleSelectFolder" />
    </div>
    <div class="workspace-main">
      <FileList
        :files="workspaceStore.files"
        @upload="handleUpload"
        @download="handleDownload"
        @delete="handleDelete"
        @select="handleSelectFile"
      />
    </div>
    <div class="workspace-preview">
      <FilePreview :file="activeFile" />
    </div>
  </div>
</template>

<script setup>
import { onMounted, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { ElMessage } from 'element-plus';

import FileList from '@/components/FileList.vue';
import FilePreview from '@/components/FilePreview.vue';
import WorkspaceTree from '@/components/WorkspaceTree.vue';
import { useWorkspaceStore } from '@/stores/workspace';
import { downloadWunderWorkspaceFile } from '@/api/workspace';

const workspaceStore = useWorkspaceStore();
const activeFile = ref(null);
const route = useRoute();

const init = async () => {
  activeFile.value = null;
  await workspaceStore.loadFolders();
  await workspaceStore.loadFiles('');
};

const handleSelectFolder = async (folderId) => {
  activeFile.value = null;
  await workspaceStore.loadFiles(folderId);
};

const createFolder = async () => {
  try {
    await workspaceStore.createFolder({ name: `新建文件夹-${Date.now()}` });
    ElMessage.success('已创建文件夹');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '创建失败');
  }
};

const handleUpload = async (file) => {
  try {
  await workspaceStore.uploadFile(file, workspaceStore.activePath);
  ElMessage.success('上传成功');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '上传失败');
  }
};

const handleDownload = async (file) => {
  activeFile.value = file;
  const response = await downloadWunderWorkspaceFile({ path: file.path });
  const url = window.URL.createObjectURL(response.data);
  const link = document.createElement('a');
  link.href = url;
  link.download = file.name;
  link.click();
  window.URL.revokeObjectURL(url);
};

const handleDelete = async (file) => {
  activeFile.value = null;
  await workspaceStore.deleteFile(file.path);
};

const handleSelectFile = (file) => {
  activeFile.value = file;
};

onMounted(init);

// 路由切换时刷新列表，兼容演示模式切换
watch(
  () => route.path,
  () => {
    init();
  }
);
</script>
