<template>
  <component
    :is="linkTag"
    v-bind="linkProps"
    class="portal-card"
    :class="{ 'is-disabled': isDisabled, 'is-external': isExternal }"
  >
    <div class="portal-card-head">
      <div class="portal-card-icon">
        <svg
          v-if="iconName === 'chat'"
          class="portal-card-icon-svg"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path d="M5 5h14a3 3 0 0 1 3 3v7a3 3 0 0 1-3 3H9l-4 3v-3H5a3 3 0 0 1-3-3V8a3 3 0 0 1 3-3z" />
        </svg>
        <svg
          v-else-if="iconName === 'workspace'"
          class="portal-card-icon-svg"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path d="M4 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7z" />
        </svg>
        <svg
          v-else-if="iconName === 'settings'"
          class="portal-card-icon-svg"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <circle cx="12" cy="12" r="3.2" />
          <path
            d="M19.4 15a7.8 7.8 0 0 0 .1-1l2-1.1-2-3.5-2.2.4a7.7 7.7 0 0 0-.8-.6l-.4-2.3h-4l-.4 2.3a6.8 6.8 0 0 0-.8.6l-2.2-.4-2 3.5 2 1.1a7.8 7.8 0 0 0 .1 1l-2 1.1 2 3.5 2.2-.4c.3.2.5.4.8.6l.4 2.3h4l.4-2.3c.3-.2.6-.4.8-.6l2.2.4 2-3.5-2-1.1z"
          />
        </svg>
        <svg
          v-else-if="iconName === 'user'"
          class="portal-card-icon-svg"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <circle cx="12" cy="8" r="4" />
          <path d="M4 20a8 8 0 0 1 16 0" />
        </svg>
        <svg
          v-else-if="iconName === 'docs'"
          class="portal-card-icon-svg"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path d="M5 4h10l4 4v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z" />
          <path d="M15 4v4h4" />
          <path d="M8 12h8M8 16h8" />
        </svg>
        <svg
          v-else-if="iconName === 'status'"
          class="portal-card-icon-svg"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <path d="M4 12h4l2-4 4 8 2-4h4" />
        </svg>
        <svg
          v-else-if="iconName === 'community'"
          class="portal-card-icon-svg"
          viewBox="0 0 24 24"
          aria-hidden="true"
        >
          <circle cx="8" cy="10" r="3" />
          <circle cx="16" cy="10" r="3" />
          <path d="M3 20a5 5 0 0 1 10 0" />
          <path d="M11 20a5 5 0 0 1 10 0" />
        </svg>
        <svg v-else class="portal-card-icon-svg" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M4 12h16M12 4v16" />
        </svg>
      </div>
      <div class="portal-card-badges">
        <span v-if="module.badge" class="portal-card-badge">{{ module.badge }}</span>
        <span v-if="module.status" class="portal-card-status">{{ module.status }}</span>
      </div>
    </div>
    <div class="portal-card-title">{{ module.title }}</div>
    <div class="portal-card-desc">{{ module.description }}</div>
    <div v-if="module.tags?.length" class="portal-card-tags">
      <span v-for="tag in module.tags" :key="tag" class="portal-card-tag">
        {{ tag }}
      </span>
    </div>
    <div class="portal-card-action">
      <span>{{ actionLabel }}</span>
      <svg class="portal-card-arrow" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M5 12h12" />
        <path d="M13 6l6 6-6 6" />
      </svg>
    </div>
  </component>
</template>

<script setup>
import { computed } from 'vue';
import { RouterLink } from 'vue-router';

const props = defineProps({
  module: {
    type: Object,
    required: true
  },
  basePath: {
    type: String,
    default: '/app'
  }
});

const iconName = computed(() => props.module?.icon || 'default');
const isExternal = computed(() => props.module?.type === 'external');
const isDisabled = computed(() => props.module?.enabled === false);
const internalPath = computed(() => {
  const suffix = String(props.module?.path || '').replace(/^\//, '');
  return `${props.basePath}/${suffix}`;
});

const linkTag = computed(() => {
  if (isDisabled.value) return 'div';
  return isExternal.value ? 'a' : RouterLink;
});

const linkProps = computed(() => {
  if (isDisabled.value) {
    return { role: 'button', 'aria-disabled': 'true' };
  }
  if (isExternal.value) {
    return {
      href: props.module?.url || '#',
      target: '_blank',
      rel: 'noopener'
    };
  }
  return { to: internalPath.value };
});

const actionLabel = computed(() => {
  if (isDisabled.value) {
    return props.module?.status || '待配置';
  }
  return isExternal.value ? '打开外链' : '进入功能';
});
</script>
