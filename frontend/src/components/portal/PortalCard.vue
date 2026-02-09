<template>
  <component
    :is="linkTag"
    v-bind="linkProps"
    class="portal-card"
    :class="{ 'is-disabled': isDisabled, 'is-external': isExternal }"
  >
    <div class="portal-card-head">
      <div class="portal-card-icon">
        <i
          class="portal-card-icon-svg"
          :class="['fa-solid', iconClass]"
          aria-hidden="true"
        ></i>
      </div>
      <div class="portal-card-badges">
        <span v-if="resolvedBadge" class="portal-card-badge">{{ resolvedBadge }}</span>
        <span v-if="resolvedStatus" class="portal-card-status">{{ resolvedStatus }}</span>
      </div>
    </div>
    <div class="portal-card-title">{{ resolvedTitle }}</div>
    <div class="portal-card-desc">{{ resolvedDescription }}</div>
    <div v-if="resolvedTags.length" class="portal-card-tags">
      <span v-for="tag in resolvedTags" :key="tag" class="portal-card-tag">
        {{ tag }}
      </span>
    </div>
    <div class="portal-card-action">
      <span>{{ actionLabel }}</span>
      <i class="fa-solid fa-arrow-right portal-card-arrow" aria-hidden="true"></i>
    </div>
  </component>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { RouterLink } from 'vue-router';

import { useI18n } from '@/i18n';

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
const { t } = useI18n();
const ICON_CLASS_MAP = {
  chat: 'fa-comments',
  workspace: 'fa-folder-open',
  settings: 'fa-sliders',
  user: 'fa-user',
  docs: 'fa-file-lines',
  status: 'fa-chart-line',
  community: 'fa-users',
  default: 'fa-plus'
};
const iconClass = computed(() => ICON_CLASS_MAP[iconName.value] || ICON_CLASS_MAP.default);
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

const resolvedTitle = computed(() =>
  props.module?.titleKey ? t(props.module.titleKey) : props.module?.title || ''
);

const resolvedDescription = computed(() =>
  props.module?.descriptionKey ? t(props.module.descriptionKey) : props.module?.description || ''
);

const resolvedBadge = computed(() =>
  props.module?.badgeKey ? t(props.module.badgeKey) : props.module?.badge || ''
);

const resolvedStatus = computed(() =>
  props.module?.statusKey ? t(props.module.statusKey) : props.module?.status || ''
);

const resolvedTags = computed(() => {
  if (Array.isArray(props.module?.tagKeys)) {
    return props.module.tagKeys.map((key) => t(key));
  }
  return Array.isArray(props.module?.tags) ? props.module.tags : [];
});

const actionLabel = computed(() => {
  if (isDisabled.value) {
    return resolvedStatus.value || t('portal.card.pending');
  }
  return isExternal.value ? t('portal.card.action.external') : t('portal.card.action.internal');
});
</script>
