<template>
  <div class="input-container">
    <div v-if="showUploadArea" class="upload-preview">
      <div class="upload-preview-list">
        <div v-for="attachment in attachments" :key="attachment.id" class="upload-preview-item">
          <i
            v-if="attachment.type === 'image'"
            class="fa-solid fa-image upload-preview-icon"
            aria-hidden="true"
          ></i>
          <i v-else class="fa-solid fa-file-lines upload-preview-icon" aria-hidden="true"></i>
          <span class="upload-preview-name" :title="attachment.name">{{ attachment.name }}</span>
          <button
            class="upload-preview-remove"
            type="button"
            :title="t('common.remove')"
            :aria-label="t('common.remove')"
            @click="removeAttachment(attachment.id)"
          >
            <i class="fa-solid fa-xmark upload-preview-remove-icon" aria-hidden="true"></i>
          </button>
        </div>
      </div>
      <div v-if="attachmentBusy > 0" class="upload-preview-status">
        {{ t('chat.attachments.processing', { count: attachmentBusy }) }}
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
        :title="t('chat.attachments.upload')"
        :aria-label="t('chat.attachments.upload')"
        :disabled="attachmentBusy > 0"
        @click="triggerUpload"
      >
        <i class="fa-solid fa-paperclip input-icon" aria-hidden="true"></i>
      </button>
      <button
        class="input-icon-btn send-btn"
        type="button"
        :disabled="!canSendOrStop"
        :title="loading ? t('common.stop') : t('chat.input.send')"
        :aria-label="loading ? t('common.stop') : t('chat.input.send')"
        @click="handleSendOrStop"
      >
        <i
          v-if="loading"
          class="fa-solid fa-stop input-icon input-icon-fill"
          aria-hidden="true"
        ></i>
        <i v-else class="fa-solid fa-paper-plane input-icon input-icon-fill" aria-hidden="true"></i>
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
import { useI18n } from '@/i18n';

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
const { t } = useI18n();

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
  props.inquiryActive ? t('chat.input.inquiryPlaceholder') : t('chat.input.placeholder')
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
    reader.onerror = () => reject(new Error(t('chat.attachments.imageReadFailed')));
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
        throw new Error(t('chat.attachments.imageEmpty'));
      }
      attachments.value.push({
        id: buildAttachmentId(),
        type: 'image',
        name: filename,
        content: dataUrl,
        mime_type: file.type || ''
      });
      ElMessage.success(t('chat.attachments.imageAdded', { name: filename }));
      return;
    }

    const extension = resolveFileExtension(filename);
    if (!extension || !DOC_EXTENSIONS.includes(`.${extension}`)) {
      throw new Error(
        t('chat.attachments.unsupportedType', { ext: extension || t('common.unknown') })
      );
    }

    const response = await convertChatAttachment(file);
    const payload = response?.data?.data || {};
    const content = typeof payload.content === 'string' ? payload.content : '';
    if (!content.trim()) {
      throw new Error(t('chat.attachments.emptyResult'));
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
      ElMessage.warning(t('chat.attachments.convertWarning', { message: warnings[0] }));
    } else {
      ElMessage.success(t('chat.attachments.fileParsed', { name: payload.name || filename }));
    }
  } catch (error) {
    ElMessage.error(resolveUploadError(error, t('chat.attachments.processFailed')));
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
    ElMessage.warning(t('chat.attachments.busy'));
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
