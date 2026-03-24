<template>
  <div v-if="visible" class="messenger-message-feedback-actions">
    <button
      class="messenger-message-footer-copy messenger-message-feedback-btn"
      :class="{ 'is-active': selectedVote === 'up' }"
      type="button"
      :title="t('chat.message.feedbackUp')"
      :aria-label="t('chat.message.feedbackUp')"
      :disabled="isDisabled"
      @click="submitVote('up')"
    >
      <i class="fa-solid fa-thumbs-up" aria-hidden="true"></i>
    </button>
    <button
      class="messenger-message-footer-copy messenger-message-feedback-btn"
      :class="{ 'is-active': selectedVote === 'down' }"
      type="button"
      :title="t('chat.message.feedbackDown')"
      :aria-label="t('chat.message.feedbackDown')"
      :disabled="isDisabled"
      @click="submitVote('down')"
    >
      <i class="fa-solid fa-thumbs-down" aria-hidden="true"></i>
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue';
import { ElMessage } from 'element-plus';

import { useI18n } from '@/i18n';
import { useChatStore } from '@/stores/chat';
import {
  normalizeMessageFeedback,
  resolveMessageHistoryId,
  type MessageFeedbackVote
} from '@/utils/messageFeedback';

const props = defineProps<{
  message?: Record<string, unknown> | null;
}>();

const { t } = useI18n();
const chatStore = useChatStore();
const submitting = ref(false);

const selectedVote = computed<MessageFeedbackVote | ''>(() => {
  const feedback = normalizeMessageFeedback(props.message?.feedback);
  return feedback?.vote || '';
});

const historyId = computed<number>(() => resolveMessageHistoryId(props.message));

const visible = computed<boolean>(() => {
  const role = String(props.message?.role || '').trim().toLowerCase();
  return role === 'assistant' && props.message?.isGreeting !== true;
});

const isDisabled = computed<boolean>(() => {
  if (submitting.value) return true;
  if (selectedVote.value) return true;
  return !String(chatStore.activeSessionId || '').trim();
});

const submitVote = async (vote: MessageFeedbackVote) => {
  if (isDisabled.value) return;
  const sessionId = String(chatStore.activeSessionId || '').trim();
  if (!sessionId) return;
  submitting.value = true;
  try {
    let targetHistoryId = historyId.value;
    if (targetHistoryId <= 0) {
      targetHistoryId = await chatStore.ensureAssistantMessageHistoryId(
        sessionId,
        props.message || null
      );
    }
    if (!Number.isFinite(targetHistoryId) || targetHistoryId <= 0) {
      ElMessage.warning(t('chat.message.feedbackFailed'));
      return;
    }
    const payload = await chatStore.submitMessageFeedback(sessionId, targetHistoryId, vote);
    if (!payload) {
      ElMessage.warning(t('chat.message.feedbackFailed'));
      return;
    }
    ElMessage.success(
      vote === 'up'
        ? t('chat.message.feedbackUpSuccess')
        : t('chat.message.feedbackDownSuccess')
    );
  } catch (error) {
    const status = Number((error as { response?: { status?: number } } | null)?.response?.status || 0);
    if (status === 409) {
      ElMessage.info(t('chat.message.feedbackLocked'));
      return;
    }
    ElMessage.warning(t('chat.message.feedbackFailed'));
  } finally {
    submitting.value = false;
  }
};
</script>
