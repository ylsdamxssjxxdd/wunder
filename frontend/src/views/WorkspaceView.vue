<template>
  <div class="workspace-view">
      <div class="workspace-sidebar">
        <div class="workspace-actions">
        <el-button type="primary" size="small" @click="createFolder">
          {{ t('workspace.panel.newFolder') }}
        </el-button>
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
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

const workspaceStore = useWorkspaceStore();
const activeFile = ref(null);
const route = useRoute();
const { t } = useI18n();

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
    await workspaceStore.createFolder({ name: `${t('workspace.panel.newFolder')}-${Date.now()}` });
    ElMessage.success(t('workspace.panel.folderCreated'));
  } catch (error) {
    showApiError(error, t('workspace.panel.createFailed'));
  }
};

const handleUpload = async (file) => {
  try {
  await workspaceStore.uploadFile(file, workspaceStore.activePath);
  ElMessage.success(t('workspace.panel.uploadSuccess'));
  } catch (error) {
    showApiError(error, t('workspace.panel.uploadFailed'));
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