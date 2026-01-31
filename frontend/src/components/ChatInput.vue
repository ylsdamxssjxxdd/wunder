<template>
  <div class="chat-input">
    <el-input
      v-model="text"
      type="textarea"
      :rows="3"
      :placeholder="t('chat.input.placeholder')"
      @keyup.enter.exact.prevent="submit"
    />
    <div class="chat-actions">
      <el-button type="primary" :loading="loading" @click="submit">
        {{ t('chat.input.send') }}
      </el-button>
    </div>
  </div>
</template>

<script setup>
import { ref } from 'vue';
import { useI18n } from '@/i18n';

defineProps({
  loading: {
    type: Boolean,
    default: false
  }
});

const emit = defineEmits(['send']);
const text = ref('');
const { t } = useI18n();

const submit = () => {
  const value = text.value.trim();
  if (!value) return;
  emit('send', value);
  text.value = '';
};
</script>
