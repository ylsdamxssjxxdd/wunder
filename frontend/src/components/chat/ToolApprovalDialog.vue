<template>
  <el-dialog
    :model-value="visible"
    width="560px"
    top="12vh"
    :show-close="false"
    :close-on-click-modal="false"
    :close-on-press-escape="false"
    append-to-body
    @closed="busy = false"
  >
    <template #header>
      <div class="tool-approval-header">
        <i class="fa-solid fa-shield-halved" aria-hidden="true"></i>
        <span>{{ t('chat.approval.title') }}</span>
      </div>
    </template>
    <div v-if="approval" class="tool-approval-body">
      <div class="tool-approval-summary">{{ approval.summary }}</div>
      <div class="tool-approval-meta">
        <span v-if="approval.tool">{{ t('chat.approval.tool') }}: {{ approval.tool }}</span>
        <span v-if="approval.kind">{{ t('chat.approval.kind') }}: {{ approvalKindLabel }}</span>
      </div>
      <pre v-if="detailText" class="tool-approval-detail">{{ detailText }}</pre>
    </div>
    <template #footer>
      <el-button :disabled="busy" @click="submit('deny')">{{ t('chat.approval.deny') }}</el-button>
      <el-button type="primary" :disabled="busy" @click="submit('approve_once')">
        {{ t('chat.approval.once') }}
      </el-button>
      <el-button type="success" :disabled="busy" @click="submit('approve_session')">
        {{ t('chat.approval.session') }}
      </el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { useChatStore } from '@/stores/chat';
import { useI18n } from '@/i18n';
import { showApiError } from '@/utils/apiError';

const chatStore = useChatStore();
const { t } = useI18n();

const busy = ref(false);
const approval = computed(() => chatStore.activeApproval);
const visible = computed(() => Boolean(approval.value));

const approvalKindLabel = computed(() => {
  const raw = String(approval.value?.kind || '').trim().toLowerCase();
  if (raw === 'exec') return t('chat.approval.kind.exec');
  if (raw === 'patch') return t('chat.approval.kind.patch');
  return raw || '-';
});

const detailText = computed(() => {
  const value = approval.value?.detail;
  if (value === null || value === undefined || value === '') {
    return '';
  }
  if (typeof value === 'string') {
    return value;
  }
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
});

const submit = async (decision: 'approve_once' | 'approve_session' | 'deny') => {
  if (!approval.value || busy.value) return;
  busy.value = true;
  try {
    await chatStore.respondApproval(decision, approval.value.approval_id);
    if (decision !== 'deny') {
      ElMessage.success(t('chat.approval.sent'));
    }
  } catch (error) {
    showApiError(error, t('chat.approval.sendFailed'));
  } finally {
    busy.value = false;
  }
};
</script>

<style scoped>
.tool-approval-header {
  display: inline-flex;
  align-items: center;
  gap: 10px;
  font-weight: 700;
}

.tool-approval-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.tool-approval-summary {
  font-size: 14px;
  line-height: 1.7;
  font-weight: 600;
  color: var(--el-text-color-primary);
  word-break: break-word;
}

.tool-approval-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.tool-approval-detail {
  margin: 0;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid var(--el-border-color-light);
  background: var(--el-fill-color-lighter);
  font-size: 12px;
  line-height: 1.6;
  max-height: 220px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
}
</style>
