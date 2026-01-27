<template>
  <div class="portal-side-card">
    <div class="portal-side-header">
      <div>
        <div class="portal-side-title">外链入口</div>
        <div class="portal-side-desc">统一配置的外部页面入口</div>
      </div>
      <div class="portal-side-meta">共 {{ displayCount }} 项</div>
    </div>
    <div class="portal-side-scroll">
      <div v-if="filteredGroups.length === 0" class="portal-side-empty">
        {{ normalizedQuery ? '未找到匹配的外链入口' : '暂无外链配置' }}
      </div>
      <div
        v-for="group in filteredGroups"
        :key="group.id"
        class="portal-side-group"
      >
        <div class="portal-side-group-title">
          <span>{{ group.title }}</span>
          <span class="portal-side-group-count">{{ group.items.length }}</span>
        </div>
        <div class="portal-side-grid">
          <PortalCard
            v-for="item in group.items"
            :key="item.id"
            :module="item"
            :base-path="basePath"
            class="portal-card--compact"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<script setup>
import { computed } from 'vue';

import PortalCard from '@/components/portal/PortalCard.vue';

const props = defineProps({
  groups: {
    type: Array,
    default: () => []
  },
  query: {
    type: String,
    default: ''
  },
  basePath: {
    type: String,
    default: '/app'
  }
});

const normalizedQuery = computed(() => String(props.query || '').trim().toLowerCase());

const matchesQuery = (item, query) => {
  if (!query) return true;
  const source = [
    item.title,
    item.description,
    ...(item.tags || [])
  ]
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
  return source.includes(query);
};

const filteredGroups = computed(() => {
  const query = normalizedQuery.value;
  return props.groups
    .map((group) => {
      const items = (group.items || []).filter((item) => matchesQuery(item, query));
      return { ...group, items };
    })
    .filter((group) => group.items.length > 0);
});

const totalCount = computed(() =>
  props.groups.reduce((sum, group) => sum + (group.items?.length || 0), 0)
);

const filteredCount = computed(() =>
  filteredGroups.value.reduce((sum, group) => sum + (group.items?.length || 0), 0)
);

const displayCount = computed(() => (normalizedQuery.value ? filteredCount.value : totalCount.value));
</script>
