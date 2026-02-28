<template>
  <el-tooltip
    v-if="visible"
    :show-after="160"
    :disabled="!fullText"
    :teleported="true"
    placement="bottom-start"
    popper-class="thinking-tooltip-popper"
    :enterable="true"
  >
    <template #content>
      <div class="thinking-tooltip">{{ fullText }}</div>
    </template>
    <div class="message-thinking">
      <span class="message-thinking-label">{{ thinkingLabel }}</span>
      <div ref="marqueeRef" class="message-thinking-marquee">
        <span class="message-thinking-track">
          {{ displayText }}
        </span>
      </div>
    </div>
  </el-tooltip>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';

import { useI18n } from '@/i18n';

const props = defineProps({
  content: {
    type: String,
    default: ''
  },
  streaming: {
    type: Boolean,
    default: false
  }
});

const { t } = useI18n();

const stripTrailingTimestamp = (text) => {
  const value = String(text || '').trim();
  if (!value) return '';
  // 去除尾部时间信息，避免占位导致显示干扰
  const patterns = [
    // 兼容毫秒与时区尾缀，例如 2025-12-30 12:42:20.183 / 2025-12-30T12:42:20.183Z / 2025-12-30 12:42:20+08:00
    /\s*[\(\[（【]?\s*\d{4}[/-]\d{1,2}[/-]\d{1,2}[ T]\d{1,2}:\d{2}:\d{2}(?:\.\d{1,3})?(?:\s*(?:Z|UTC|GMT)(?:[+-]\d{1,2}(?::?\d{2})?)?|\s*[+-]\d{2}:?\d{2})?\s*[\)\]】）]?\s*$/i,
    // 仅日期结尾
    /\s*[\(\[（【]?\s*\d{4}[/-]\d{1,2}[/-]\d{1,2}\s*[\)\]】）]?\s*$/i,
    // 仅时间结尾（含毫秒/时区）
    /\s*[\(\[（【]?\s*\d{1,2}:\d{2}:\d{2}(?:\.\d{1,3})?(?:\s*(?:Z|UTC|GMT)(?:[+-]\d{1,2}(?::?\d{2})?)?|\s*[+-]\d{2}:?\d{2})?\s*[\)\]】）]?\s*$/i
  ];
  let cleaned = value;
  for (let i = 0; i < 2; i += 1) {
    const before = cleaned;
    for (const pattern of patterns) {
      cleaned = cleaned.replace(pattern, '').trim();
    }
    cleaned = cleaned.replace(/[|·•—–-]+\s*$/, '').trim();
    if (cleaned === before) {
      break;
    }
  }
  return cleaned;
};

// 将思考内容压缩成单行，避免跑马灯换行
const displayText = computed(() => {
  const normalized = String(props.content || '').replace(/\s+/g, ' ').trim();
  return stripTrailingTimestamp(normalized);
});

const fullText = computed(() => stripTrailingTimestamp(String(props.content || '')));

const visible = computed(() => Boolean(displayText.value));

const thinkingLabel = computed(() =>
  props.streaming ? t('chat.thinking') : t('chat.thinkingDone')
);

const marqueeRef = ref(null);

const scrollToTail = (behavior = 'auto') => {
  const target = marqueeRef.value;
  if (!target) return;
  const maxScroll = Math.max(0, target.scrollWidth - target.clientWidth);
  if (typeof target.scrollTo === 'function') {
    target.scrollTo({ left: maxScroll, behavior });
  } else {
    target.scrollLeft = maxScroll;
  }
};

const syncMarquee = (behavior = 'auto') => {
  // 等待渲染后将跑马灯滚到最新内容，保持与流式节奏一致
  nextTick(() => scrollToTail(behavior));
};

watch(displayText, () => {
  if (displayText.value) {
    syncMarquee('auto');
  }
});

watch(
  () => props.streaming,
  (value) => {
    if (!value) {
      syncMarquee('auto');
    }
  }
);

onMounted(() => {
  if (visible.value) {
    syncMarquee('auto');
  }
});
</script>


