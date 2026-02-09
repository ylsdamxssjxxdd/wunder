<template>
  <section class="portal-section">
    <div class="portal-section-header">
      <div>
        <div class="portal-section-title">{{ sectionTitle }}</div>
        <div class="portal-section-desc">{{ sectionDescription }}</div>
      </div>
      <div class="portal-section-meta">
        {{ t('portal.section.count', { count: section.items.length }) }}
      </div>
    </div>
    <div class="portal-grid" :class="{ 'portal-grid--compact': compact }">
      <PortalCard
        v-for="item in section.items"
        :key="item.id"
        :module="item"
        :base-path="basePath"
        :class="compact ? 'portal-card--compact' : ''"
      />
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed } from 'vue';

import PortalCard from '@/components/portal/PortalCard.vue';
import { useI18n } from '@/i18n';

const props = defineProps({
  section: {
    type: Object,
    required: true
  },
  basePath: {
    type: String,
    default: '/app'
  },
  compact: {
    type: Boolean,
    default: false
  }
});

const { t } = useI18n();

const sectionTitle = computed(() =>
  props.section?.titleKey ? t(props.section.titleKey) : props.section?.title || ''
);

const sectionDescription = computed(() =>
  props.section?.descriptionKey ? t(props.section.descriptionKey) : props.section?.description || ''
);
</script>
