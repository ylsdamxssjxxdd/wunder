<template>
  <div class="input-container">
    <div v-if="showUploadArea" class="upload-preview">
      <div class="upload-preview-list">
        <div v-for="attachment in attachments" :key="attachment.id" class="upload-preview-item">
          <svg
            v-if="attachment.type === 'image'"
            class="upload-preview-icon"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <rect x="3" y="5" width="18" height="14" rx="2" />
            <path d="M8 13l2.5-3 3.5 4 2.5-3 3.5 5" />
          </svg>
          <svg v-else class="upload-preview-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M7 3h7l5 5v13a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z" />
            <path d="M14 3v5h5" />
          </svg>
          <span class="upload-preview-name" :title="attachment.name">{{ attachment.name }}</span>
          <button
            class="upload-preview-remove"
            type="button"
            title="移除"
            aria-label="移除"
            @click="removeAttachment(attachment.id)"
          >
            <svg class="upload-preview-remove-icon" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M6 6l12 12M18 6l-12 12" />
            </svg>
          </button>
        </div>
      </div>
      <div v-if="attachmentBusy > 0" class="upload-preview-status">
        正在处理 {{ attachmentBusy }} 个附件...
      </div>
    </div>

    <div
      class="input-box"
      :class="{ dragover: dragActive }"
      @dragenter="handleDragEnter"
      @dragover="handleDragOver"
      @dragleave="handleDragLeave"
      @drop="handleDrop"
    >
      <textarea
        v-model="inputText"
        ref="inputRef"
        :placeholder="inputPlaceholder"
        rows="1"
        @input="resizeInput"
        @keydown.enter.exact.prevent="handleSend"
      />
      <button
        class="input-icon-btn upload-btn"
        type="button"
        title="上传附件"
        aria-label="上传附件"
        :disabled="attachmentBusy > 0"
        @click="triggerUpload"
      >
        <svg class="input-icon" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M8 12.5l5.5-5.5a3 3 0 1 1 4.2 4.2l-7 7a5 5 0 0 1-7.1-7.1l7.1-7.1" />
        </svg>
      </button>
      <button
        class="input-icon-btn send-btn"
        type="button"
        :disabled="!canSendOrStop"
        :title="loading ? '终止' : '发送'"
        :aria-label="loading ? '终止' : '发送'"
        @click="handleSendOrStop"
      >
        <svg v-if="loading" class="input-icon input-icon-fill" viewBox="0 0 24 24" aria-hidden="true">
          <rect x="6" y="6" width="12" height="12" rx="2" />
        </svg>
        <svg v-else class="input-icon input-icon-fill" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M20 12l-16-8 6 8-6 8 16-8z" />
        </svg>
      </button>
    </div>

    <input
      ref="uploadInputRef"
      type="file"
      hidden
      multiple
      :accept="uploadAccept"
      @change="handleUploadInput"
    />
  </div>
</template>

<script setup>
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { ElMessage } from 'element-plus';

import { convertChatAttachment } from '@/api/chat';

const props = defineProps({
  loading: {
    type: Boolean,
    default: false
  },
  demoMode: {
    type: Boolean,
    default: false
  },
  inquiryActive: {
    type: Boolean,
    default: false
  },
  inquirySelection: {
    type: Array,
    default: () => []
  }
});

const emit = defineEmits(['send', 'stop']);

const inputText = ref('');
const inputRef = ref(null);
const uploadInputRef = ref(null);
const attachments = ref([]);
const attachmentBusy = ref(0);
const dragActive = ref(false);
const dragCounter = ref(0);

const IMAGE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp', 'svg']);
const DOC_EXTENSIONS = [
  '.txt',
  '.md',
  '.markdown',
  '.html',
  '.htm',
  '.py',
  '.c',
  '.cpp',
  '.cc',
  '.h',
  '.hpp',
  '.json',
  '.js',
  '.ts',
  '.css',
  '.ini',
  '.cfg',
  '.log',
  '.doc',
  '.docx',
  '.odt',
  '.pptx',
  '.odp',
  '.xlsx',
  '.ods',
  '.wps',
  '.et',
  '.dps'
];
const uploadAccept = ['image/*', ...DOC_EXTENSIONS].join(',');
const INPUT_MAX_HEIGHT = 180;

const showUploadArea = computed(() => attachments.value.length > 0 || attachmentBusy.value > 0);
const hasInquirySelection = computed(
  () => Array.isArray(props.inquirySelection) && props.inquirySelection.length > 0
);
const inputPlaceholder = computed(() =>
  props.inquiryActive ? '选择选项或输入文本后点击发送' : '输入消息...'
);
const canSendOrStop = computed(() => {
  if (props.loading) return true;
  if (attachmentBusy.value > 0) return false;
  return (
    Boolean(inputText.value.trim()) ||
    attachments.value.length > 0 ||
    hasInquirySelection.value
  );
});

const buildAttachmentId = () => `${Date.now()}_${Math.random().toString(16).slice(2)}`;

const resolveUploadError = (error, fallback) =>
  error?.response?.data?.detail || error?.message || fallback;

const resolveFileExtension = (filename) => {
  const parts = String(filename || '').trim().split('.');
  if (parts.length < 2) return '';
  return parts.pop().toLowerCase();
};

const isImageFile = (file) => {
  if (file?.type && file.type.startsWith('image/')) {
    return true;
  }
  const ext = resolveFileExtension(file?.name);
  return ext ? IMAGE_EXTENSIONS.has(ext) : false;
};

const readFileAsDataUrl = (file) =>
  new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ''));
    reader.onerror = () => reject(new Error('图片读取失败'));
    reader.readAsDataURL(file);
  });

// 发送时只保留 Wunder 需要的字段，避免 UI 状态混入请求
const buildAttachmentPayload = () =>
  attachments.value
    .filter((item) => String(item?.content || '').trim())
    .map((item) => {
      const payload = {
        type: item.type,
        name: item.name,
        content: item.content
      };
      if (item.mime_type) {
        payload.mime_type = item.mime_type;
      }
      return payload;
    });

const resizeInput = () => {
  const el = inputRef.value;
  if (!el) return;
  el.style.height = 'auto';
  const nextHeight = Math.min(el.scrollHeight, INPUT_MAX_HEIGHT);
  el.style.height = `${nextHeight}px`;
  el.style.overflowY = el.scrollHeight > INPUT_MAX_HEIGHT ? 'auto' : 'hidden';
};

const resetInputHeight = () => {
  const el = inputRef.value;
  if (!el) return;
  el.style.height = 'auto';
  el.style.overflowY = 'hidden';
};

const triggerUpload = () => {
  if (!uploadInputRef.value) return;
  uploadInputRef.value.value = '';
  uploadInputRef.value.click();
};

const hasFileDrag = (event) => Array.from(event?.dataTransfer?.types || []).includes('Files');

const handleDragEnter = (event) => {
  if (!hasFileDrag(event)) return;
  event.preventDefault();
  dragCounter.value += 1;
  dragActive.value = true;
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'copy';
  }
};

const handleDragOver = (event) => {
  if (!hasFileDrag(event)) return;
  event.preventDefault();
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = 'copy';
  }
};

const handleDragLeave = (event) => {
  if (!hasFileDrag(event)) return;
  dragCounter.value = Math.max(0, dragCounter.value - 1);
  if (dragCounter.value === 0) {
    dragActive.value = false;
  }
};

const handleDrop = async (event) => {
  if (!hasFileDrag(event)) return;
  event.preventDefault();
  dragCounter.value = 0;
  dragActive.value = false;
  const files = Array.from(event.dataTransfer?.files || []);
  if (!files.length) return;
  for (const file of files) {
    await handleAttachmentSelection(file);
  }
};

// 附件处理遵循 Wunder 调试面板：图片走 data URL，文件先转 Markdown
const handleAttachmentSelection = async (file) => {
  if (!file) return;
  const filename = file.name || 'upload';
  attachmentBusy.value += 1;
  try {
    if (isImageFile(file)) {
      const dataUrl = await readFileAsDataUrl(file);
      if (!dataUrl) {
        throw new Error('图片内容为空');
      }
      attachments.value.push({
        id: buildAttachmentId(),
        type: 'image',
        name: filename,
        content: dataUrl,
        mime_type: file.type || ''
      });
      ElMessage.success(`已附加图片：${filename}`);
      return;
    }

    const extension = resolveFileExtension(filename);
    if (!extension || !DOC_EXTENSIONS.includes(`.${extension}`)) {
      throw new Error(`不支持的文件类型：.${extension || '未知'}`);
    }

    const response = await convertChatAttachment(file);
    const payload = response?.data?.data || {};
    const content = typeof payload.content === 'string' ? payload.content : '';
    if (!content.trim()) {
      throw new Error('解析结果为空');
    }
    attachments.value.push({
      id: buildAttachmentId(),
      type: 'file',
      name: payload.name || filename,
      content,
      mime_type: file.type || '',
      converter: payload.converter || ''
    });
    const warnings = Array.isArray(payload.warnings) ? payload.warnings : [];
    if (warnings.length) {
      ElMessage.warning(`文件转换存在警告：${warnings[0]}`);
    } else {
      ElMessage.success(`已解析文件：${payload.name || filename}`);
    }
  } catch (error) {
    ElMessage.error(resolveUploadError(error, '附件处理失败'));
  } finally {
    attachmentBusy.value = Math.max(0, attachmentBusy.value - 1);
  }
};

const handleUploadInput = async (event) => {
  const files = Array.from(event.target.files || []);
  if (!files.length) return;
  for (const file of files) {
    await handleAttachmentSelection(file);
  }
};

const removeAttachment = (id) => {
  attachments.value = attachments.value.filter((item) => item.id !== id);
};

const clearAttachments = () => {
  attachments.value = [];
};

const handleSend = async () => {
  if (props.loading) return;
  // 附件解析未完成时禁止发送，避免请求缺少必要内容
  if (attachmentBusy.value > 0) {
    ElMessage.warning('附件处理中，请稍后再发送。');
    return;
  }
  const content = inputText.value.trim();
  const payloadAttachments = buildAttachmentPayload();
  if (!content && payloadAttachments.length === 0 && !hasInquirySelection.value) return;
  emit('send', { content, attachments: payloadAttachments });
  inputText.value = '';
  resetInputHeight();
  clearAttachments();
};

const handleSendOrStop = async () => {
  if (props.loading) {
    emit('stop');
    return;
  }
  await handleSend();
};

onMounted(async () => {
  await nextTick();
  resizeInput();
});

// 演示模式切换时清理附件缓存，避免状态残留
watch(
  () => props.demoMode,
  (value) => {
    if (value) {
      clearAttachments();
    }
  }
);
</script>
