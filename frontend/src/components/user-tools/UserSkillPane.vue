<template>
  <div class="user-tools-pane">
    <div class="list-header">
      <label>{{ t('userTools.skills.title') }}</label>
      <div class="header-actions">
        <button class="user-tools-btn secondary compact" type="button" @click="triggerUpload">
          {{ t('userTools.skills.action.upload') }}
        </button>
        <button class="user-tools-btn secondary compact" type="button" @click="reloadSkills">
          {{ t('common.refresh') }}
        </button>
      </div>
    </div>
    <div class="tips">{{ t('userTools.skills.tip') }}</div>
    <input ref="uploadInputRef" type="file" accept=".zip" hidden @change="handleUpload" />

    <div class="skills-list">
      <div v-if="!skills.length" class="empty-text">{{ t('userTools.skills.list.empty') }}</div>
      <div
        v-for="skill in skills"
        :key="skill.name"
        class="skill-item tool-item-dual"
        @click="openSkillDetail(skill)"
      >
        <label class="tool-check" @click.stop>
          <input type="checkbox" :checked="skill.enabled" @change="toggleEnable(skill, $event.target.checked)" />
          <span>{{ t('userTools.action.enable') }}</span>
        </label>
        <label class="tool-check" @click.stop>
          <input type="checkbox" :checked="skill.shared" @change="toggleShared(skill, $event.target.checked)" />
          <span>{{ t('userTools.action.share') }}</span>
        </label>
        <label class="tool-item-info">
          <strong>{{ skill.name }}</strong>
          <span class="muted">{{ buildSkillDesc(skill) }}</span>
        </label>
      </div>
    </div>

    <el-dialog
      v-model="detailVisible"
      class="user-tools-dialog user-tools-subdialog"
      width="640px"
      :show-close="false"
      append-to-body
    >
      <template #header>
        <div class="user-tools-header">
          <div class="user-tools-title">{{ detailTitle }}</div>
          <button class="icon-btn" type="button" @click="detailVisible = false">×</button>
        </div>
      </template>
      <div class="user-tools-detail">
        <div class="detail-line">
          <span class="label">{{ t('userTools.skills.detail.pathLabel') }}</span>
          <span>{{ detailMeta || '-' }}</span>
        </div>
        <pre class="detail-schema">{{ detailContent }}</pre>
      </div>
      <template #footer>
        <el-button class="user-tools-footer-btn" @click="detailVisible = false">
          {{ t('common.close') }}
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { onBeforeUnmount, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserSkillContent, fetchUserSkills, saveUserSkills, uploadUserSkillZip } from '@/api/userTools';
import { useI18n } from '@/i18n';

const props = defineProps({
  visible: {
    type: Boolean,
    default: false
  },
  active: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['status']);
const { t } = useI18n();

const skills = ref([]);
const loaded = ref(false);
const loading = ref(false);
const saveTimer = ref(null);

const uploadInputRef = ref(null);

const detailVisible = ref(false);
const detailTitle = ref('');
const detailMeta = ref('');
const detailContent = ref('');
let detailVersion = 0;

const emitStatus = (message) => {
  emit('status', message || '');
};

const buildSkillDesc = (skill) => {
  const parts = [];
  if (skill.description) {
    parts.push(skill.description);
  }
  if (skill.path) {
    parts.push(skill.path);
  }
  return parts.join(' · ') || t('common.noDescription');
};

const loadSkills = async () => {
  if (loading.value) return;
  loading.value = true;
  try {
    const { data } = await fetchUserSkills();
    const payload = data?.data || {};
    skills.value = Array.isArray(payload.skills) ? payload.skills : [];
    loaded.value = true;
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || t('userTools.skills.loadFailed'));
  } finally {
    loading.value = false;
  }
};

const saveSkills = async () => {
  emitStatus(t('userTools.saving'));
  try {
    const enabled = skills.value.filter((skill) => skill.enabled).map((skill) => skill.name);
    const shared = skills.value.filter((skill) => skill.shared).map((skill) => skill.name);
    const { data } = await saveUserSkills({ enabled, shared });
    const payload = data?.data || {};
    skills.value = Array.isArray(payload.skills) ? payload.skills : skills.value;
    emitStatus(t('userTools.autoSaved'));
  } catch (error) {
    emitStatus(t('userTools.saveFailed', { message: error.message || t('common.requestFailed') }));
    ElMessage.error(error.response?.data?.detail || t('userTools.skills.saveFailed'));
  }
};

// 输入即保存，节流避免频繁写入
const scheduleSave = () => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
  saveTimer.value = setTimeout(() => {
    saveTimer.value = null;
    saveSkills();
  }, 600);
};

const toggleEnable = (skill, checked) => {
  skill.enabled = checked;
  if (!skill.enabled) {
    skill.shared = false;
  }
  scheduleSave();
};

const toggleShared = (skill, checked) => {
  skill.shared = checked;
  if (skill.shared) {
    skill.enabled = true;
  }
  scheduleSave();
};

const triggerUpload = () => {
  if (!uploadInputRef.value) return;
  uploadInputRef.value.value = '';
  uploadInputRef.value.click();
};

const handleUpload = async () => {
  const file = uploadInputRef.value?.files?.[0];
  if (!file) return;
  try {
    await uploadUserSkillZip(file);
    await loadSkills();
    ElMessage.success(t('userTools.skills.upload.success'));
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || t('userTools.skills.upload.failed'));
  }
};

const reloadSkills = async () => {
  try {
    await loadSkills();
    ElMessage.success(t('userTools.skills.refresh.success'));
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || t('userTools.skills.refresh.failed'));
  }
};

const openSkillDetail = async (skill) => {
  if (!skill?.name) return;
  const currentVersion = ++detailVersion;
  detailTitle.value = skill.name || t('userTools.skills.detail.title');
  detailMeta.value = skill.path || '';
  detailContent.value = t('common.loading');
  detailVisible.value = true;
  try {
    const { data } = await fetchUserSkillContent(skill.name);
    const payload = data?.data || {};
    if (currentVersion !== detailVersion) {
      return;
    }
    detailContent.value = payload.content || t('userTools.skills.detail.empty');
  } catch (error) {
    if (currentVersion !== detailVersion) {
      return;
    }
    detailContent.value = t('common.loadFailed', { message: error.message || t('common.requestFailed') });
  }
};

watch(
  () => props.visible,
  (value) => {
    if (value && !loaded.value) {
      loadSkills();
    }
  }
);

onBeforeUnmount(() => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
});
</script>
