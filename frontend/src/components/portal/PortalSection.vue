<template>
  <section class="portal-section">
    <div class="portal-section-header">
      <div>
        <div class="portal-section-title">{{ section.title }}</div>
        <div class="portal-section-desc">{{ section.description }}</div>
      </div>
      <div class="portal-section-meta">共 {{ section.items.length }} 项</div>
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

<script setup>
import PortalCard from '@/components/portal/PortalCard.vue';

defineProps({
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
</script>
