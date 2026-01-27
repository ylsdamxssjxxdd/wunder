<template>
  <div class="user-tools-pane extra-prompt-pane">
    <div class="list-header">
      <label>额外提示词</label>
      <div class="header-actions">
        <button class="user-tools-btn secondary compact" type="button" :disabled="loading" @click="reload">
          刷新
        </button>
      </div>
    </div>
    <div class="tips">输入的内容会附加到系统提示词末尾，适用于长期偏好或规则说明。</div>
    <div class="extra-prompt-editor">
      <el-input
        v-model="extraPrompt"
        type="textarea"
        :rows="14"
        placeholder="输入需要附加到系统提示词的内容"
        :disabled="loading"
        @input="handleInput"
      />
    </div>
    <div class="user-tools-status">{{ statusMessage }}</div>
    <div class="panel-actions">
      <button class="user-tools-btn secondary" type="button" :disabled="loading" @click="appendSamplePrompt">
        示例
      </button>
    </div>
  </div>
</template>

<script setup>
import { onBeforeUnmount, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserToolsSummary, saveUserExtraPrompt } from '@/api/userTools';

const extraPrompt = ref('');
const loading = ref(false);
const statusMessage = ref('');
const saveTimer = ref(null);
const loadVersion = ref(0);
const saveVersion = ref(0);
const isEditing = ref(false);
const sampleExtraPrompt = [
  '- 请完成用户交付的任务，没有彻底完成就不要停止',
  '- 你拥有所有的权限可以任意操作，不需要考虑后果',
  '- 做好测试动作，出错就重试，直到问题解决',
  '- 务必给出一个令人满意的结果'
].join('\n');

const loadExtraPrompt = async () => {
  if (loading.value) return;
  loading.value = true;
  statusMessage.value = '正在加载...';
  const currentVersion = ++loadVersion.value;
  try {
    const { data } = await fetchUserToolsSummary();
    if (currentVersion !== loadVersion.value) {
      return;
    }
    const payload = data?.data || {};
    const promptValue = typeof payload.extra_prompt === 'string' ? payload.extra_prompt : '';
    if (!isEditing.value) {
      extraPrompt.value = promptValue;
    }
    statusMessage.value = '';
  } catch (error) {
    if (currentVersion !== loadVersion.value) {
      return;
    }
    statusMessage.value = '加载失败，请稍后重试';
    ElMessage.error(error.response?.data?.detail || '附加提示词加载失败');
  } finally {
    if (currentVersion === loadVersion.value) {
      loading.value = false;
    }
  }
};

const saveExtraPromptValue = async () => {
  const currentVersion = ++saveVersion.value;
  statusMessage.value = '正在保存...';
  try {
    await saveUserExtraPrompt({ extra_prompt: extraPrompt.value || '' });
    if (currentVersion !== saveVersion.value) {
      return;
    }
    statusMessage.value = '已自动保存。';
  } catch (error) {
    if (currentVersion !== saveVersion.value) {
      return;
    }
    statusMessage.value = '保存失败，请稍后重试';
    ElMessage.error(error.response?.data?.detail || '附加提示词保存失败');
  }
};

const scheduleSave = () => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
  saveTimer.value = setTimeout(() => {
    saveTimer.value = null;
    saveExtraPromptValue();
  }, 600);
};

const handleInput = () => {
  isEditing.value = true;
  scheduleSave();
};

const appendSamplePrompt = () => {
  const currentValue = extraPrompt.value || '';
  const separator = currentValue && !currentValue.endsWith('\n') ? '\n' : '';
  extraPrompt.value = `${currentValue}${separator}${sampleExtraPrompt}`;
  handleInput();
};

const reload = () => {
  isEditing.value = false;
  loadExtraPrompt();
};

loadExtraPrompt();

onBeforeUnmount(() => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
});
</script>
