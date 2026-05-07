<template>
  <div
    class="message-goal-divider"
    role="separator"
    aria-live="polite"
    :title="objective || undefined"
  >
    <span class="message-goal-divider-track" aria-hidden="true"></span>
    <span class="message-goal-divider-label">
      {{ label }}
    </span>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from '@/i18n';

const props = withDefaults(
  defineProps<{
    objective?: string;
  }>(),
  {
    objective: ''
  }
);

const { t } = useI18n();

const label = computed(() => {
  const objective = String(props.objective || '').trim();
  if (!objective) {
    return t('chat.goal.started');
  }
  return `${t('chat.goal.started')}：${objective}`;
});
</script>

<style scoped>
.message-goal-divider {
  width: 100%;
  margin: 18px 0 20px;
  padding: 0;
}

.message-goal-divider-track {
  display: block;
  height: 1px;
  width: 100%;
  background: linear-gradient(
    90deg,
    rgba(20, 184, 166, 0),
    rgba(20, 184, 166, 0.56) 22%,
    rgba(20, 184, 166, 0.56) 78%,
    rgba(20, 184, 166, 0)
  );
}

.message-goal-divider-label {
  display: block;
  margin-top: 7px;
  text-align: center;
  font-size: 11px;
  line-height: 1.45;
  letter-spacing: 0.01em;
  color: rgba(13, 148, 136, 0.94);
  font-weight: 700;
}
</style>
