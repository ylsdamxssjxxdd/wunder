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

<script setup lang="ts">
import { onMounted, ref, watch } from 'vue';
import { useRoute } from 'vue-router';
import { ElLoading, ElMessage } from 'element-plus';
import type { AxiosProgressEvent } from 'axios';

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

const formatBytes = (size: number) => {
  const value = Number(size) || 0;
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(1)} MB`;
  return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GB`;
};

const buildTransferText = (label: string, event: AxiosProgressEvent, fallback: string) => {
  const loaded = Number(event.loaded) || 0;
  const total = Number.isFinite(event.total) ? Number(event.total) : 0;
  if (total > 0) {
    const percent = Math.max(0, Math.min(100, Math.round((loaded / total) * 100)));
    return `${label} ${percent}% (${formatBytes(loaded)} / ${formatBytes(total)})`;
  }
  if (loaded > 0) {
    return `${label} ${formatBytes(loaded)}`;
  }
  return fallback;
};

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
  const loading = ElLoading.service({
    lock: false,
    text: t('workspace.upload.progress.loading', { label: t('common.upload') })
  });
  try {
    await workspaceStore.uploadFile(file, workspaceStore.activePath, {
      onUploadProgress: (event) => {
        loading.setText(
          buildTransferText(
            t('common.upload'),
            event,
            t('workspace.upload.progress.loading', { label: t('common.upload') })
          )
        );
      }
    });
    ElMessage.success(t('workspace.panel.uploadSuccess'));
  } catch (error) {
    showApiError(error, t('workspace.panel.uploadFailed'));
  } finally {
    loading.close();
  }
};

const handleDownload = async (file) => {
  activeFile.value = file;
  const loading = ElLoading.service({
    lock: false,
    text: t('workspace.download.preparing', { name: file.name || t('workspace.download.defaultName') })
  });
  try {
    const response = await downloadWunderWorkspaceFile(
      { path: file.path },
      {
        onDownloadProgress: (event) => {
          loading.setText(
            buildTransferText(
              t('common.download'),
              event,
              t('workspace.download.progress.loading', { label: t('common.download') })
            )
          );
        }
      }
    );
    const url = window.URL.createObjectURL(response.data);
    const link = document.createElement('a');
    link.href = url;
    link.download = file.name;
    link.click();
    window.URL.revokeObjectURL(url);
    ElMessage.success(t('workspace.download.success'));
  } catch (error) {
    showApiError(error, t('workspace.download.failed'));
  } finally {
    loading.close();
  }
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
