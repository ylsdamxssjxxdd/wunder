<template>
  <div class="chat-input">
    <el-input
      v-model="text"
      type="textarea"
      :rows="3"
      placeholder="请输入内容"
      @keyup.enter.exact.prevent="submit"
    />
    <div class="chat-actions">
      <el-button type="primary" :loading="loading" @click="submit">发送</el-button>
    </div>
  </div>
</template>

<script setup>
import { ref } from 'vue';

defineProps({
  loading: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['send']);
const text = ref('');

const submit = () => {
  const value = text.value.trim();
  if (!value) return;
  emit('send', value);
  text.value = '';
};
</script>
