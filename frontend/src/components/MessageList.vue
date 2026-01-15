<template>
  <el-scrollbar class="message-scroll">
    <div v-for="(message, index) in messages" :key="index" class="message-item">
      <div :class="['message-bubble', message.role === 'user' ? 'user' : 'assistant']">
        <span class="message-role">{{ message.role === 'user' ? '你' : '智能体' }}</span>
        <div class="message-content">{{ message.content }}</div>
      </div>
    </div>
  </el-scrollbar>
</template>

<script setup>
defineProps({
  messages: {
    type: Array,
    default: () => []
  }
});
</script>
