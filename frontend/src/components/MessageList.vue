<template>
  <el-scrollbar class="message-scroll">
    <div v-for="(message, index) in messages" :key="index" class="message-item">
      <div :class="['message-bubble', message.role === 'user' ? 'user' : 'assistant']">
        <span class="message-role">{{
          message.role === 'user' ? t('chat.message.user') : t('chat.message.assistant')
        }}</span>
        <div class="message-content">{{ message.content }}</div>
      </div>
    </div>
  </el-scrollbar>
</template>

<script setup lang="ts">
import { useI18n } from '@/i18n';

type MessageItem = {
  role?: string;
  content?: string;
};

defineProps<{
  messages: MessageItem[];
}>();

const { t } = useI18n();
</script>
