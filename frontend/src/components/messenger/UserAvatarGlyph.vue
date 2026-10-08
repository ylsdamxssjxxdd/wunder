<template>
  <svg
    class="messenger-user-avatar-icon"
    :style="{ width: `${resolvedSize}px`, height: `${resolvedSize}px` }"
    viewBox="0 0 24 24"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    aria-hidden="true"
  >
    <template v-if="normalizedGlyph === 'check'">
      <circle cx="12" cy="12" r="8.5" />
      <path d="M8.6 12.3L10.9 14.6L15.5 9.7" />
    </template>
    <template v-else-if="normalizedGlyph === 'spark'">
      <path d="M12 4.5L13.7 8.5L17.8 10L13.7 11.5L12 15.5L10.3 11.5L6.2 10L10.3 8.5L12 4.5Z" />
      <path d="M16.2 15.4L16.9 17.1L18.6 17.8L16.9 18.5L16.2 20.2L15.5 18.5L13.8 17.8L15.5 17.1L16.2 15.4Z" />
    </template>
    <template v-else-if="normalizedGlyph === 'target'">
      <circle cx="12" cy="12" r="7.2" />
      <circle cx="12" cy="12" r="2.6" />
      <path d="M12 2.8V5.4M12 18.6V21.2M2.8 12H5.4M18.6 12H21.2" />
    </template>
    <template v-else-if="normalizedGlyph === 'idea'">
      <path
        d="M8.7 10.1C8.7 8.3 10.1 6.8 11.9 6.8C13.7 6.8 15.1 8.3 15.1 10.1C15.1 11.5 14.5 12.4 13.7 13.2C13.2 13.7 12.9 14.2 12.8 14.8H11C10.9 14.2 10.6 13.6 10.1 13.2C9.3 12.4 8.7 11.5 8.7 10.1Z"
      />
      <path d="M10.2 17.4H13.6M10.8 19.6H13" />
    </template>
    <template v-else-if="normalizedGlyph === 'code'">
      <path d="M9.1 8.2L6.2 12L9.1 15.8M14.9 8.2L17.8 12L14.9 15.8M13.2 6.4L10.8 17.6" />
    </template>
    <template v-else-if="normalizedGlyph === 'pen'">
      <path
        d="M7.1 16.8L7.9 13.6L15.9 5.6C16.6 4.9 17.7 4.9 18.4 5.6V5.6C19.1 6.3 19.1 7.4 18.4 8.1L10.4 16.1L7.1 16.8Z"
      />
      <path d="M14.2 7.4L16.6 9.8" />
    </template>
    <template v-else-if="normalizedGlyph === 'briefcase'">
      <rect x="4.6" y="8.6" width="14.8" height="9.8" rx="2.1" />
      <path d="M9.1 8.6V7.5C9.1 6.8 9.7 6.2 10.4 6.2H13.6C14.3 6.2 14.9 6.8 14.9 7.5V8.6" />
      <path d="M4.6 12.4H19.4" />
    </template>
    <template v-else-if="normalizedGlyph === 'shield'">
      <path d="M12 4.3L18 6.7V11.2C18 14.9 15.6 17.8 12 19.7C8.4 17.8 6 14.9 6 11.2V6.7L12 4.3Z" />
      <path d="M9.3 11.9L11.1 13.7L14.8 10" />
    </template>
    <template v-else>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M8.6 12.3L10.9 14.6L15.5 9.7" />
    </template>
  </svg>
</template>

<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    glyph?: string;
    size?: number;
  }>(),
  {
    glyph: 'check',
    size: 16
  }
);

const normalizeAvatarGlyph = (value: unknown): string => {
  const text = String(value || '')
    .trim()
    .toLowerCase();
  if (!text) return 'check';
  const aliasMap: Record<string, string> = {
    check: 'check',
    spark: 'spark',
    target: 'target',
    idea: 'idea',
    code: 'code',
    pen: 'pen',
    briefcase: 'briefcase',
    shield: 'shield',
    'fa-user': 'check',
    'fa-user-astronaut': 'spark',
    'fa-rocket': 'target',
    'fa-lightbulb': 'idea',
    'fa-code': 'code',
    'fa-pen': 'pen',
    'fa-briefcase': 'briefcase',
    'fa-shield-halved': 'shield'
  };
  return aliasMap[text] || 'check';
};

const normalizedGlyph = computed(() => normalizeAvatarGlyph(props.glyph));
const resolvedSize = computed(() => {
  const parsed = Number(props.size);
  if (!Number.isFinite(parsed)) return 16;
  return Math.max(10, Math.min(28, Math.round(parsed)));
});
</script>
