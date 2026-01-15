<template>
  <div class="user-tools-pane">
    <div class="list-header">
      <label>技能管理</label>
      <div class="header-actions">
        <button class="user-tools-btn secondary compact" type="button" @click="triggerUpload">
          上传技能包
        </button>
        <button class="user-tools-btn secondary compact" type="button" @click="reloadSkills">
          刷新
        </button>
      </div>
    </div>
    <div class="tips">技能名称会以 user_id@技能名 展示，勾选共享即可让其他用户使用。</div>
    <input ref="uploadInputRef" type="file" accept=".zip" hidden @change="handleUpload" />

    <div class="skills-list">
      <div v-if="!skills.length" class="empty-text">未发现技能，请先上传技能包。</div>
      <div
        v-for="skill in skills"
        :key="skill.name"
        class="skill-item tool-item-dual"
        @click="openSkillDetail(skill)"
      >
        <label class="tool-check" @click.stop>
          <input type="checkbox" :checked="skill.enabled" @change="toggleEnable(skill, $event.target.checked)" />
          <span>启用</span>
        </label>
        <label class="tool-check" @click.stop>
          <input type="checkbox" :checked="skill.shared" @change="toggleShared(skill, $event.target.checked)" />
          <span>共享</span>
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
          <span class="label">路径</span>
          <span>{{ detailMeta || '-' }}</span>
        </div>
        <pre class="detail-schema">{{ detailContent }}</pre>
      </div>
      <template #footer>
        <el-button class="user-tools-footer-btn" @click="detailVisible = false">关闭</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup>
import { onBeforeUnmount, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserSkillContent, fetchUserSkills, saveUserSkills, uploadUserSkillZip } from '@/api/userTools';

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
  return parts.join(' · ') || '暂无描述';
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
    ElMessage.error(error.response?.data?.detail || '技能加载失败');
  } finally {
    loading.value = false;
  }
};

const saveSkills = async () => {
  emitStatus('正在保存...');
  try {
    const enabled = skills.value.filter((skill) => skill.enabled).map((skill) => skill.name);
    const shared = skills.value.filter((skill) => skill.shared).map((skill) => skill.name);
    const { data } = await saveUserSkills({ enabled, shared });
    const payload = data?.data || {};
    skills.value = Array.isArray(payload.skills) ? payload.skills : skills.value;
    emitStatus('已自动保存。');
  } catch (error) {
    emitStatus(`保存失败：${error.message || '请求失败'}`);
    ElMessage.error(error.response?.data?.detail || '自建技能保存失败');
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
    ElMessage.success('技能上传完成并已刷新。');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '技能上传失败');
  }
};

const reloadSkills = async () => {
  try {
    await loadSkills();
    ElMessage.success('技能列表已刷新。');
  } catch (error) {
    ElMessage.error(error.response?.data?.detail || '技能刷新失败');
  }
};

const openSkillDetail = async (skill) => {
  if (!skill?.name) return;
  const currentVersion = ++detailVersion;
  detailTitle.value = skill.name || '技能详情';
  detailMeta.value = skill.path || '';
  detailContent.value = '加载中...';
  detailVisible.value = true;
  try {
    const { data } = await fetchUserSkillContent(skill.name);
    const payload = data?.data || {};
    if (currentVersion !== detailVersion) {
      return;
    }
    detailContent.value = payload.content || '（无内容）';
  } catch (error) {
    if (currentVersion !== detailVersion) {
      return;
    }
    detailContent.value = `加载失败：${error.message || '请求失败'}`;
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
