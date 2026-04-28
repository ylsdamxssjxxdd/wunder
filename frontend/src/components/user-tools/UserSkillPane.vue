<template>
  <div class="user-tools-pane">
    <div class="list-header">
      <label>{{ t('userTools.skills.title') }}</label>
      <div class="header-actions">
        <button class="user-tools-btn secondary btn-with-icon" type="button" @click="triggerUpload">
          <i class="fa-solid fa-plus" aria-hidden="true"></i>
          <span>{{ t('userTools.skills.action.upload') }}</span>
        </button>
        <button
          class="user-tools-btn secondary btn-with-icon"
          type="button"
          :disabled="!activeSkill"
          @click="exportSkill"
        >
          <i class="fa-solid fa-file-zipper" aria-hidden="true"></i>
          <span>{{ t('userTools.skills.action.export') }}</span>
        </button>
        <button class="user-tools-btn secondary btn-with-icon" type="button" @click="reloadSkills">
          <i class="fa-solid fa-arrows-rotate" aria-hidden="true"></i>
          <span>{{ t('common.refresh') }}</span>
        </button>
        <div v-if="status" class="user-tools-status list-status">{{ status }}</div>
      </div>
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

      <div class="management-detail skill-detail">
        <div class="skill-detail-pane">
          <div class="detail-header">
            <div>
              <div class="detail-title">{{ detailTitle }}</div>
              <div class="muted skill-path-line" :title="detailMeta || undefined">{{ detailMeta }}</div>
            </div>
          </div>
          <div class="skills-section-title">{{ t('userTools.skills.detail.structure') }}</div>
          <div class="skill-file-tree">
            <div v-if="fileTreeMessage" class="empty-text">{{ fileTreeMessage }}</div>
            <template v-else>
              <div
                v-for="entry in fileEntries"
                :key="entry.path"
                class="skill-tree-item"
                :class="{
                  'is-dir': entry.kind === 'dir',
                  'is-file': entry.kind !== 'dir',
                  'is-active': entry.kind !== 'dir' && entry.path === activeFile
                }"
                :style="{ paddingLeft: `${8 + entry.depth * 14}px` }"
                :title="entry.path"
                @click="entry.kind !== 'dir' && selectSkillFile(entry.path)"
              >
                <i
                  class="fa-solid"
                  :class="entry.kind === 'dir' ? 'fa-folder' : 'fa-file-lines'"
                  aria-hidden="true"
                ></i>
                <span class="skill-tree-name">{{ entry.name }}</span>
              </div>
            </template>
          </div>
        </div>
        <div class="skill-detail-pane">
          <div class="detail-header">
            <div>
              <div class="detail-title">{{ t('userTools.skills.editor.title') }}</div>
              <div class="muted skill-path-line" :title="activeFile || undefined">
                {{ activeFile || t('userTools.skills.file.unselected') }}
              </div>
              <div v-if="activeSkillReadonly" class="muted">{{ t('userTools.skills.readonlyHint') }}</div>
            </div>
            <div class="detail-actions">
              <button
                class="user-tools-btn btn-with-icon btn-compact icon-only"
                type="button"
                :disabled="editorDisabled"
                :title="t('common.save')"
                :aria-label="t('common.save')"
                @click="saveSkillFile"
              >
                <i class="fa-solid fa-floppy-disk" aria-hidden="true"></i>
              </button>
            </div>
          </div>
          <div class="skill-editor-body" :class="{ 'is-disabled': editorDisabled }">
            <textarea
              v-model="fileContent"
              class="skill-editor-text"
              :placeholder="t('userTools.skills.file.placeholder')"
              :disabled="editorDisabled"
              spellcheck="false"
              autocorrect="off"
              autocomplete="off"
              autocapitalize="off"
            ></textarea>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { ElMessage, ElMessageBox } from 'element-plus';

import {
  deleteUserSkill,
  exportUserSkillArchive,
  fetchUserSkillFile,
  fetchUserSkillFiles,
  fetchUserSkills,
  saveUserSkillFile,
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

const emit = defineEmits(['loading-change']);

const { t } = useI18n();

const skills = ref([]);
const selectedIndex = ref(-1);
const loaded = ref(false);
const loading = ref(false);
const deleteLoading = ref(false);

const fileEntries = ref([]);
const activeFile = ref('');
const fileContent = ref('');
const fileTreeMessage = ref('');
const editorLocked = ref(true);

const uploadInputRef = ref(null);

let detailVersion = 0;
let fileVersion = 0;

const emitLoadingChange = (value) => {
  emit('loading-change', value === true);
};

const syncUserSkillsCatalog = (action: string) => {
  invalidateAllUserToolsCaches();
  emitUserToolsUpdated({ scope: 'skills', action });
};

const normalizeSkillDisplayPath = (value) => {
  let normalized = String(value || '').trim();
  if (!normalized) {
    return '';
  }
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
  if (!Number.isInteger(selectedIndex.value)) {
    return null;
  }
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

const resolveSkillSource = (skill) => {
  if (skill?.source === 'builtin' || skill?.builtin === true || skill?.readonly === true) {
    return 'builtin';
  }
  return 'custom';
};

const isSkillReadonly = (skill) => resolveSkillSource(skill) === 'builtin';

const activeSkillReadonly = computed(() => isSkillReadonly(activeSkill.value));
const editorDisabled = computed(
  () => editorLocked.value || !activeFile.value || activeSkillReadonly.value
);

const buildSkillDesc = (skill) => {
  const parts = [];
  if (skill.description) {
    parts.push(skill.description);
  }
  const displayPath = normalizeSkillDisplayPath(skill.path);
  if (displayPath) {
    parts.push(displayPath);
  }
  return parts.join(' · ') || t('common.noDescription');
};

const buildSkillSourceLabel = (skill) =>
  resolveSkillSource(skill) === 'builtin'
    ? t('userTools.skills.source.builtin')
    : t('userTools.skills.source.custom');

const normalizeSkillPath = (path) => String(path || '').replace(/\\/g, '/');

const resolveDefaultSkillFile = (entries) => {
  if (!Array.isArray(entries)) {
    return '';
  }
  let fallback = '';
  for (const entry of entries) {
    if (!entry || entry.kind === 'dir') {
      continue;
    }
    const path = String(entry.path || '');
    if (!path) {
      continue;
    }
    const normalized = normalizeSkillPath(path).toLowerCase();
    if (normalized === 'skill.md') {
      return path;
    }
    if (!fallback && normalized.endsWith('/skill.md')) {
      fallback = path;
    }
  }
  return fallback;
};

const setEditorDisabled = (disabled) => {
  editorLocked.value = disabled;
};

const showEditorMessage = (message) => {
  fileContent.value = message || '';
  setEditorDisabled(true);
};

const resolveErrorMessage = (error, fallback) => {
  const detail = error?.response?.data?.detail;
  if (typeof detail === 'string') {
    return detail;
  }
  if (detail && typeof detail.message === 'string') {
    return detail.message;
  }
  if (typeof error?.message === 'string' && error.message.trim()) {
    return error.message;
  }
  return fallback;
};

const refreshFileTreeMessage = () => {
  if (!activeSkill.value) {
    fileTreeMessage.value = t('userTools.skills.files.unselected');
    return;
  }
  if (!fileEntries.value.length) {
    fileTreeMessage.value = t('userTools.skills.files.empty');
    return;
  }
  fileTreeMessage.value = '';
};

const resetSkillDetailState = () => {
  detailVersion += 1;
  fileVersion += 1;
  fileEntries.value = [];
  activeFile.value = '';
  showEditorMessage('');
};

const clearSkillSelection = () => {
  selectedIndex.value = -1;
  resetSkillDetailState();
  refreshFileTreeMessage();
};

type LoadSkillsOptions = {
  refreshDetail?: boolean;
};

const buildFileEntries = (entries) =>
  entries
    .map((entry) => {
      const path = String(entry?.path || '');
      if (!path) return null;
      const segments = path.split('/');
      return {
        path,
        kind: entry?.kind === 'dir' ? 'dir' : 'file',
        depth: Math.max(0, segments.length - 1),
        name: segments[segments.length - 1] || path
      };
    })
    .filter(Boolean);

const loadSkills = async ({ refreshDetail }: LoadSkillsOptions = {}) => {
  if (loading.value) return;
  loading.value = true;
  emitLoadingChange(true);
  try {
    const { data } = await fetchUserSkills();
    const payload = data?.data || {};
    const list = Array.isArray(payload.skills) ? payload.skills : [];
    const activeName = activeSkill.value?.name || '';
    skills.value = list;
    loaded.value = true;
    if (activeName) {
      const index = list.findIndex((item) => item.name === activeName);
      if (index >= 0) {
        selectedIndex.value = index;
        if (refreshDetail) {
          await selectSkill(list[index], index);
        } else {
          refreshFileTreeMessage();
        }
        return;
      }
    }
    clearSkillSelection();
  } catch (error) {
    showApiError(error, t('userTools.skills.loadFailed'));
  } finally {
    loading.value = false;
    emitLoadingChange(false);
  }
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
  try {
    const response = await exportUserSkillArchive(skill.name);
    const filename = getFilenameFromHeaders(response.headers, `${skill.name}.zip`);
    const objectUrl = URL.createObjectURL(response.data);
    saveObjectUrlAsFile(objectUrl, filename);
    window.setTimeout(() => URL.revokeObjectURL(objectUrl), 0);
    ElMessage.success(t('userTools.skills.export.success'));
  } catch (error) {
    showApiError(error, t('userTools.skills.export.failed'));
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
  try {
    await uploadUserSkillZip(file);
    await loadSkills({ refreshDetail: true });
    syncUserSkillsCatalog('upload');
    ElMessage.success(t('userTools.skills.upload.success'));
  } catch (error) {
    showApiError(error, t('userTools.skills.upload.failed'));
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

const selectSkill = async (skill, index) => {
  if (!skill) {
    clearSkillSelection();
    return;
  }
  selectedIndex.value = index;
  resetSkillDetailState();
  fileTreeMessage.value = t('common.loading');
  const currentVersion = detailVersion;
  try {
    const { data } = await fetchUserSkillFiles(skill.name);
    if (currentVersion !== detailVersion) {
      return;
    }
    const payload = data?.data || {};
    const entries = Array.isArray(payload.entries) ? payload.entries : [];
    fileEntries.value = buildFileEntries(entries);
    refreshFileTreeMessage();
    const defaultFile = resolveDefaultSkillFile(entries);
    if (defaultFile) {
      await selectSkillFile(defaultFile);
    }
  } catch (error) {
    if (currentVersion !== detailVersion) {
      return;
    }
    fileTreeMessage.value = t('userTools.skills.files.loadFailed', {
      message: resolveErrorMessage(error, t('common.requestFailed'))
    });
  }
};

const selectSkillFile = async (filePath) => {
  const skill = activeSkill.value;
  if (!skill) {
    ElMessage.warning(t('userTools.skills.file.selectSkillRequired'));
    return;
  }
  if (!filePath) {
    ElMessage.warning(t('userTools.skills.file.selectRequired'));
    return;
  }
  activeFile.value = filePath;
  showEditorMessage(t('common.loading'));
  const currentVersion = ++fileVersion;
  try {
    const { data } = await fetchUserSkillFile(skill.name, filePath);
    const payload = data?.data || {};
    if (currentVersion !== fileVersion) {
      return;
    }
    fileContent.value = payload.content || '';
    setEditorDisabled(false);
  } catch (error) {
    if (currentVersion !== fileVersion) {
      return;
    }
    showEditorMessage(
      t('userTools.skills.file.readFailed', {
        message: resolveErrorMessage(error, t('common.requestFailed'))
      })
    );
  }
};

const saveSkillFile = async () => {
  const skill = activeSkill.value;
  if (!skill) {
    ElMessage.warning(t('userTools.skills.file.selectSkillRequired'));
    return;
  }
  if (isSkillReadonly(skill)) {
    ElMessage.warning(t('userTools.skills.file.readonly'));
    return;
  }
  if (!activeFile.value) {
    ElMessage.warning(t('userTools.skills.file.selectRequired'));
    return;
  }
  try {
    const { data } = await saveUserSkillFile({
      name: skill.name,
      path: activeFile.value,
      content: fileContent.value
    });
    const payload = data?.data || {};
    if (payload.reloaded) {
      await loadSkills({ refreshDetail: true });
      syncUserSkillsCatalog('save');
    }
    ElMessage.success(t('userTools.skills.file.saveSuccess'));
  } catch (error) {
    ElMessage.error(
      t('userTools.skills.file.saveFailed', {
        message: resolveErrorMessage(error, t('common.requestFailed'))
      })
    );
  }
};

const removeSkillFromList = async (skillName) => {
  const removedIndex = skills.value.findIndex((item) => item?.name === skillName);
  if (removedIndex < 0) {
    return;
  }
  const deletingActive = removedIndex === selectedIndex.value;
  skills.value = skills.value.filter((_, index) => index !== removedIndex);
  if (!skills.value.length) {
    clearSkillSelection();
    return;
  }
  if (deletingActive) {
    const nextIndex = Math.min(removedIndex, skills.value.length - 1);
    await selectSkill(skills.value[nextIndex], nextIndex);
    return;
  }
  if (selectedIndex.value > removedIndex) {
    selectedIndex.value -= 1;
  }
  refreshFileTreeMessage();
};

const deleteSkill = async (skill) => {
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
  } catch (error) {
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
        message: resolveErrorMessage(error, t('common.requestFailed'))
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
