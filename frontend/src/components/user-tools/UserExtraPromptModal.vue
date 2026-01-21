<template>
  <el-dialog
    v-model="visibleProxy"
    class="user-tools-dialog user-tools-subdialog user-tools-quick"
    width="760px"
    top="8vh"
    :show-close="false"
    :close-on-click-modal="false"
    append-to-body
  >
    <template #header>
      <div class="user-tools-header">
        <div class="user-tools-title">额外提示设置</div>
        <button class="icon-btn" type="button" @click="close">×</button>
      </div>
    </template>

    <div class="user-tools-pane">
      <div class="list-header">
        <label>附加提示词</label>
        <div class="header-actions">
          <button class="user-tools-btn secondary compact" type="button" :disabled="loading" @click="reload">
            刷新
          </button>
        </div>
      </div>
      <div class="tips">输入的内容会追加到系统提示词末尾。</div>
      <div class="extra-prompt-editor">
        <el-input
          v-model="extraPrompt"
          type="textarea"
          :rows="16"
          placeholder="输入需要附加到系统提示词的内容"
          :disabled="loading"
          @input="handleInput"
        />
      </div>
    </div>

    <div class="user-tools-status">{{ statusMessage }}</div>

    <template #footer>
      <el-button
        class="user-tools-footer-btn"
        :disabled="loading"
        @click="appendSamplePrompt"
      >
        示例
      </el-button>
      <el-button class="user-tools-footer-btn" @click="close">关闭</el-button>
    </template>
  </el-dialog>
</template>

<script setup>
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { fetchUserToolsSummary, saveUserExtraPrompt } from '@/api/userTools';

const props = defineProps({
  modelValue: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['update:modelValue']);

const visibleProxy = computed({
  get: () => props.modelValue,
  set: (value) => emit('update:modelValue', value)
});

const extraPrompt = ref('');
const loading = ref(false);
const statusMessage = ref('');
const saveTimer = ref(null);
const loadVersion = ref(0);
const saveVersion = ref(0);
const isEditing = ref(false);
const sampleExtraPrompt = [
  '- 请完成用户交代的任务，没有彻底完成就不要停止',
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
    statusMessage.value = `加载失败：${error.message || '请求失败'}`;
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
    statusMessage.value = `保存失败：${error.message || '请求失败'}`;
    ElMessage.error(error.response?.data?.detail || '附加提示词保存失败');
  }
};

const scheduleSave = () => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
  // 输入后自动保存，避免频繁写入
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

const close = () => {
  visibleProxy.value = false;
};

watch(
  () => props.modelValue,
  (value) => {
    if (value) {
      isEditing.value = false;
      loadExtraPrompt();
    } else {
      statusMessage.value = '';
    }
  }
);

onBeforeUnmount(() => {
  if (saveTimer.value) {
    clearTimeout(saveTimer.value);
  }
});
</script>
