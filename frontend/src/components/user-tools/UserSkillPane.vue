<template>
  <div class="user-tools-pane skill-workspace-shell">
    <div class="list-header">
      <label>{{ t('userTools.skills.title') }}</label>
      <div v-if="status" class="user-tools-status list-status">{{ status }}</div>
    </div>
    <div class="tips">{{ t('userTools.skills.tip') }}</div>
    <input
      ref="uploadInputRef"
      type="file"
      accept=".zip,.skill,.rar,.7z,.tar,.tgz,.tar.gz,.tbz2,.tar.bz2,.txz,.tar.xz"
      hidden
      @change="handleUpload"
    />

    <div class="management-layout skill-layout">
      <div class="management-list skill-sidebar">
        <div class="list-header">
          <label>{{ t('userTools.skills.list.title') }}</label>
          <div class="header-actions">
            <button
              class="user-tools-btn secondary btn-with-icon icon-only"
              type="button"
              :title="t('userTools.skills.action.upload')"
              :aria-label="t('userTools.skills.action.upload')"
              @click="triggerUpload"
            >
              <i class="fa-solid fa-plus" aria-hidden="true"></i>
            </button>
            <button
              class="user-tools-btn secondary btn-with-icon icon-only"
              type="button"
              :disabled="!activeSkill"
              :title="t('userTools.skills.action.export')"
              :aria-label="t('userTools.skills.action.export')"
              @click="exportSkill"
            >
              <i class="fa-solid fa-file-zipper" aria-hidden="true"></i>
            </button>
            <button
              class="user-tools-btn secondary btn-with-icon icon-only"
              type="button"
              :title="t('common.refresh')"
              :aria-label="t('common.refresh')"
              @click="reloadSkills"
            >
              <i class="fa-solid fa-arrows-rotate" aria-hidden="true"></i>
            </button>
          </div>
        </div>
        <div class="skills-list">
          <div v-if="!skills.length" class="empty-text">{{ t('userTools.skills.list.empty') }}</div>
          <div
            v-for="(skill, index) in skills"
            :key="skill.name || index"
            class="skill-item user-skill-item"
            :class="{ active: index === selectedIndex, 'has-delete': !isSkillReadonly(skill) }"
            @click="selectSkill(skill, index)"
          >
            <label class="tool-item-info">
              <div class="user-skill-title-line">
                <strong :title="skill.name">{{ skill.name }}</strong>
                <span class="skill-source-tag" :class="`is-${resolveSkillSource(skill)}`">
                  {{ buildSkillSourceLabel(skill) }}
                </span>
              </div>
              <span class="muted">{{ buildSkillDesc(skill) }}</span>
            </label>
            <button
              v-if="!isSkillReadonly(skill)"
              class="user-tools-btn danger btn-with-icon btn-compact icon-only user-skill-delete"
              type="button"
              :disabled="deleteLoading"
              :title="t('userTools.skills.delete.title')"
              @click.stop="deleteSkill(skill)"
            >
              <i class="fa-solid fa-trash-can" aria-hidden="true"></i>
            </button>
          </div>
        </div>
      </div>

      <div v-if="activeSkill" class="management-detail skill-detail skill-workspace-wrap">
        <div class="skill-workspace-shell chat-shell">
          <WorkspacePanel
            ref="workspacePanelRef"
            class="skill-workspace-panel"
            :title="detailTitle"
            :show-container-id="false"
            :empty-text="workspaceEmptyText"
            :file-system="activeSkillFileSystem"
            :disable-workspace-editors="true"
            @quote-path="handleQuotePath"
            @open-workspace-binding="handleOpenWorkspaceBinding"
          />
        </div>
      </div>
      <div v-else class="management-detail skill-detail skill-empty-detail">
        <div class="empty-text">{{ t('userTools.skills.files.unselected') }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import { ElLoading, ElMessage, ElMessageBox } from 'element-plus';
import type { AxiosProgressEvent } from 'axios';

import WorkspacePanel from '@/components/chat/WorkspacePanel.vue';
import {
  batchUserSkillAction,
  copyUserSkillEntry,
  createUserSkillDir,
  deleteUserSkill,
  downloadUserSkillArchive,
  downloadUserSkillFile,
  exportUserSkillArchive,
  fetchUserSkillFsContent,
  fetchUserSkills,
  moveUserSkillEntry,
  saveUserSkillFsFile,
  searchUserSkillFs,
  uploadUserSkillFsFiles,
  uploadUserSkillZip
} from '@/api/userTools';
import { showApiError } from '@/utils/apiError';
import { emitUserToolsUpdated } from '@/utils/userToolsEvents';
import { invalidateAllUserToolsCaches } from '@/utils/userToolsCache';
import { getFilenameFromHeaders, saveObjectUrlAsFile } from '@/utils/workspaceResourceCards';
import { useI18n } from '@/i18n';

const SUPPORTED_SKILL_ARCHIVE_SUFFIXES = [
  '.zip',
  '.skill',
  '.rar',
  '.7z',
  '.tar',
  '.tgz',
  '.tar.gz',
  '.tbz2',
  '.tar.bz2',
  '.txz',
  '.tar.xz'
];

const props = defineProps({
  visible: {
    type: Boolean,
    default: false
  },
  active: {
    type: Boolean,
    default: false
  },
  status: {
    type: String,
    default: ''
  }
});

const emit = defineEmits(['loading-change', 'quote-path']);

const { t } = useI18n();

const skills = ref<any[]>([]);
const selectedIndex = ref(-1);
const loaded = ref(false);
const loading = ref(false);
const deleteLoading = ref(false);
const uploadInputRef = ref<HTMLInputElement | null>(null);
const workspacePanelRef = ref<InstanceType<typeof WorkspacePanel> | null>(null);

const formatBytes = (size: number) => {
  const value = Number(size) || 0;
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  if (value < 1024 * 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(1)} MB`;
  return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GB`;
};

const buildTransferText = (
  label: string,
  event: AxiosProgressEvent,
  loadingText: string
) => {
  const loaded = Number(event.loaded) || 0;
  const total = Number.isFinite(event.total) ? Number(event.total) : 0;
  if (total > 0) {
    const percent = Math.max(0, Math.min(100, Math.round((loaded / total) * 100)));
    return `${label} ${percent}% (${formatBytes(loaded)} / ${formatBytes(total)})`;
  }
  if (loaded > 0) {
    return `${label} ${formatBytes(loaded)}`;
  }
  return loadingText;
};

const emitLoadingChange = (value: boolean) => {
  emit('loading-change', value === true);
};

const syncUserSkillsCatalog = (action: string) => {
  invalidateAllUserToolsCaches();
  emitUserToolsUpdated({ scope: 'skills', action });
};

const normalizeSkillDisplayPath = (value: unknown) => {
  let normalized = String(value || '').trim();
  if (!normalized) return '';
  normalized = normalized.replace(/^\\\\\?\\UNC\\/i, '\\\\');
  normalized = normalized.replace(/^\\\\\?\\/, '');
  normalized = normalized.replace(/^\/\/\?\//, '');
  normalized = normalized.replace(/^\/\/\.\//, '');
  normalized = normalized.replace(/\\/g, '/');
  if (/^\/[A-Za-z]:\//.test(normalized)) {
    normalized = normalized.slice(1);
  }
  return normalized;
};

const activeSkill = computed(() => {
  if (!Number.isInteger(selectedIndex.value)) return null;
  return skills.value[selectedIndex.value] || null;
});

const detailTitle = computed(() =>
  activeSkill.value?.name ? activeSkill.value.name : t('userTools.skills.detail.unselected')
);

const detailMeta = computed(() => {
  const skill = activeSkill.value;
  if (!skill) return '';
  return normalizeSkillDisplayPath(skill.path);
});

const resolveSkillSource = (skill: any) => {
  const source = String(skill?.source || '').trim();
  if (source === 'global') {
    return 'global';
  }
  if (source === 'builtin' || skill?.builtin === true || skill?.readonly === true) {
    return 'builtin';
  }
  return 'custom';
};

const isSkillReadonly = (skill: any) => resolveSkillSource(skill) !== 'custom';

const activeSkillReadonly = computed(() => isSkillReadonly(activeSkill.value));
const workspaceEmptyText = computed(() =>
  activeSkill.value ? t('userTools.skills.files.empty') : t('userTools.skills.files.unselected')
);

const activeSkillName = computed(() => String(activeSkill.value?.name || '').trim());
const activeSkillFileSystem = computed(() => ({
  key: `skill:${activeSkillName.value || 'none'}`,
  readonly: activeSkillReadonly.value,
  supportsWorkspaceEditors: false,
  withParams: (params: Record<string, unknown> = {}) => ({ ...params, name: activeSkillName.value }),
  appendFormData: (formData: FormData) => {
    formData.append('name', activeSkillName.value);
  },
  listContent: (params: Record<string, unknown>) => fetchUserSkillFsContent({ ...params, name: activeSkillName.value }),
  search: (params: Record<string, unknown>) => searchUserSkillFs({ ...params, name: activeSkillName.value }),
  upload: (formData: FormData, config: { onUploadProgress?: (event: AxiosProgressEvent) => void } = {}) =>
    uploadUserSkillFsFiles(formData, config),
  createDir: (payload: Record<string, unknown>) => createUserSkillDir({ ...payload, name: activeSkillName.value }),
  moveEntry: (payload: Record<string, unknown>) => moveUserSkillEntry({ ...payload, name: activeSkillName.value }),
  copyEntry: (payload: Record<string, unknown>) => copyUserSkillEntry({ ...payload, name: activeSkillName.value }),
  batchAction: (payload: Record<string, unknown>) => batchUserSkillAction({ ...payload, name: activeSkillName.value }),
  saveFile: (payload: Record<string, unknown>) => saveUserSkillFsFile({ ...payload, name: activeSkillName.value }),
  downloadFile: (
    params: Record<string, unknown>,
    config: { onDownloadProgress?: (event: AxiosProgressEvent) => void } = {}
  ) => downloadUserSkillFile(activeSkillName.value, String(params?.path || ''), config),
  downloadArchive: (
    params: Record<string, unknown>,
    config: { onDownloadProgress?: (event: AxiosProgressEvent) => void } = {}
  ) => downloadUserSkillArchive(activeSkillName.value, String(params?.path || ''), config)
}));

const buildSkillDesc = (skill: any) => {
  const parts = [];
  if (skill.description) {
    parts.push(skill.description);
  }
  const displayPath = normalizeSkillDisplayPath(skill.path);
  if (displayPath) {
    parts.push(displayPath);
  }
  return parts.join(' / ') || t('common.noDescription');
};

const buildSkillSourceLabel = (skill: any) =>
  t(`userTools.skills.source.${resolveSkillSource(skill)}`);

const emitWorkspaceLoading = (value: boolean) => {
  emitLoadingChange(value);
};

const triggerUpload = () => {
  if (!uploadInputRef.value) return;
  uploadInputRef.value.value = '';
  uploadInputRef.value.click();
};

const exportSkill = async () => {
  const skill = activeSkill.value;
  if (!skill?.name) {
    ElMessage.warning(t('userTools.skills.file.selectSkillRequired'));
    return;
  }
  const loading = ElLoading.service({
    lock: false,
    target: '.user-tools-dialog',
    text: t('userTools.skills.export.preparing'),
    background: 'rgba(15, 23, 42, 0.18)'
  });
  try {
    const response = await exportUserSkillArchive(skill.name, {
      onDownloadProgress: (event) => {
        loading.setText(
          buildTransferText(
            t('userTools.skills.export.progress'),
            event,
            t('userTools.skills.export.preparing')
          )
        );
      }
    });
    const filename = getFilenameFromHeaders(response.headers, `${skill.name}.zip`);
    const objectUrl = URL.createObjectURL(response.data);
    saveObjectUrlAsFile(objectUrl, filename);
    window.setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
    ElMessage.success(t('userTools.skills.export.success'));
  } catch (error) {
    showApiError(error, t('userTools.skills.export.failed'));
  } finally {
    loading.close();
  }
};

const handleUpload = async () => {
  const file = uploadInputRef.value?.files?.[0];
  if (!file) return;
  const filename = file.name || '';
  const lower = filename.toLowerCase();
  if (!SUPPORTED_SKILL_ARCHIVE_SUFFIXES.some((suffix) => lower.endsWith(suffix))) {
    ElMessage.warning(t('userTools.skills.upload.zipOnly'));
    uploadInputRef.value.value = '';
    return;
  }
  const loading = ElLoading.service({
    lock: false,
    target: '.user-tools-dialog',
    text: t('userTools.skills.upload.preparing'),
    background: 'rgba(15, 23, 42, 0.18)'
  });
  try {
    await uploadUserSkillZip(file, {
      onUploadProgress: (event) => {
        loading.setText(
          buildTransferText(
            t('userTools.skills.upload.progress'),
            event,
            t('userTools.skills.upload.preparing')
          )
        );
      }
    });
    await loadSkills({ refreshDetail: true });
    syncUserSkillsCatalog('upload');
    ElMessage.success(t('userTools.skills.upload.success'));
  } catch (error) {
    showApiError(error, t('userTools.skills.upload.failed'));
  } finally {
    loading.close();
    if (uploadInputRef.value) {
      uploadInputRef.value.value = '';
    }
  }
};

const reloadSkills = async () => {
  try {
    await loadSkills({ refreshDetail: true });
    syncUserSkillsCatalog('refresh');
    ElMessage.success(t('userTools.skills.refresh.success'));
  } catch (error) {
    showApiError(error, t('userTools.skills.refresh.failed'));
  }
};

const refreshSkillWorkspace = async () => {
  await nextTick();
  await workspacePanelRef.value?.refreshView?.({ background: false });
};

const selectSkill = async (skill: any, index: number) => {
  if (!skill) {
    selectedIndex.value = -1;
    return;
  }
  selectedIndex.value = index;
  await refreshSkillWorkspace();
};

const loadSkills = async ({ refreshDetail }: { refreshDetail?: boolean } = {}) => {
  if (loading.value) return;
  loading.value = true;
  emitWorkspaceLoading(true);
  try {
    const { data } = await fetchUserSkills();
    const payload = data?.data || {};
    const list = Array.isArray(payload.skills) ? payload.skills : [];
    const activeName = activeSkill.value?.name || '';
    skills.value = list;
    loaded.value = true;
    if (activeName) {
      const index = list.findIndex((item: any) => item.name === activeName);
      if (index >= 0) {
        selectedIndex.value = index;
        if (refreshDetail) {
          await selectSkill(list[index], index);
        }
        return;
      }
    }
    selectedIndex.value = list.length ? 0 : -1;
    if (selectedIndex.value >= 0 && refreshDetail) {
      await selectSkill(list[selectedIndex.value], selectedIndex.value);
    }
  } catch (error) {
    showApiError(error, t('userTools.skills.loadFailed'));
  } finally {
    loading.value = false;
    emitWorkspaceLoading(false);
  }
};

const handleQuotePath = (payload: { paths?: string[] } = {}) => {
  const paths = Array.isArray(payload.paths)
    ? payload.paths.map((item) => String(item || '').trim()).filter(Boolean)
    : [];
  if (!paths.length) return;
  emit('quote-path', { paths });
};

const handleOpenWorkspaceBinding = () => {
  // Skills do not use workspace binding.
};

const removeSkillFromList = async (skillName: string) => {
  const removedIndex = skills.value.findIndex((item) => item?.name === skillName);
  if (removedIndex < 0) return;
  const deletingActive = removedIndex === selectedIndex.value;
  skills.value = skills.value.filter((_, index) => index !== removedIndex);
  if (!skills.value.length) {
    selectedIndex.value = -1;
    return;
  }
  if (deletingActive) {
    const nextIndex = Math.min(removedIndex, skills.value.length - 1);
    selectedIndex.value = nextIndex;
    return;
  }
  if (selectedIndex.value > removedIndex) {
    selectedIndex.value -= 1;
  }
};

const deleteSkill = async (skill: any) => {
  if (!skill?.name) return;
  if (isSkillReadonly(skill)) {
    ElMessage.warning(t('userTools.skills.deleteBuiltinDenied'));
    return;
  }
  try {
    await ElMessageBox.confirm(
      t('userTools.skills.deleteConfirm', { name: skill.name }),
      t('common.notice'),
      {
        confirmButtonText: t('common.delete'),
        cancelButtonText: t('common.cancel'),
        type: 'warning'
      }
    );
  } catch {
    return;
  }
  deleteLoading.value = true;
  try {
    await deleteUserSkill(skill.name);
    await removeSkillFromList(skill.name);
    syncUserSkillsCatalog('delete');
    ElMessage.success(t('userTools.skills.deleted', { name: skill.name }));
  } catch (error) {
    ElMessage.error(
      t('userTools.skills.deleteFailed', {
        message: (error as Error)?.message || t('common.requestFailed')
      })
    );
  } finally {
    deleteLoading.value = false;
  }
};

watch(
  () => props.visible,
  (value) => {
    if (value && !loaded.value) {
      loadSkills({ refreshDetail: true });
    }
  },
  { immediate: true }
);

watch(
  () => props.active,
  (value) => {
    if (value) {
      void loadSkills({ refreshDetail: false });
    }
  },
  { immediate: true }
);
</script>
