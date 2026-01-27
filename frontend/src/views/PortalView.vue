<template>
  <div class="portal-shell">
    <UserTopbar
      title="功能广场"
      subtitle="选择你要使用的能力模块"
      show-search
      v-model:search="searchQuery"
    />
    <main class="portal-content">
      <section class="portal-main">
        <div class="portal-main-scroll">
          <div class="portal-hero">
            <div class="portal-hero-title">欢迎回来，{{ userName }}</div>
            <div class="portal-hero-sub">从这里开始新的任务或查看你的使用概况。</div>
          </div>

          <section v-if="filteredEntries.length" class="portal-section portal-section--flat">
            <div class="portal-section-header">
              <div>
                <div class="portal-section-title">页面入口</div>
                <div class="portal-section-desc">内部功能与外链入口统一展示</div>
              </div>
              <div class="portal-section-meta">共 {{ filteredEntries.length }} 项</div>
            </div>
            <div class="portal-grid">
              <PortalCard
                v-for="item in filteredEntries"
                :key="item.entryKey"
                :module="item"
                :base-path="basePath"
              />
            </div>
          </section>

          <div v-else class="portal-empty">
            {{ normalizedQuery ? '没有找到匹配的功能，请尝试其他关键词。' : '暂无可用入口。' }}
          </div>
        </div>
      </section>
    </main>
  </div>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue';
import { useRoute } from 'vue-router';

import PortalCard from '@/components/portal/PortalCard.vue';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { externalLinkGroups } from '@/config/external-links';
import { portalEntries } from '@/config/portal';
import { useAuthStore } from '@/stores/auth';

const route = useRoute();
const authStore = useAuthStore();
const searchQuery = ref('');

const basePath = computed(() => (route.path.startsWith('/demo') ? '/demo' : '/app'));
const userName = computed(() => authStore.user?.username || '访客');

const normalizedQuery = computed(() => searchQuery.value.trim().toLowerCase());

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

const internalEntries = computed(() =>
  portalEntries.map((item) => ({
    ...item,
    entryKey: `internal-${item.id}`
  }))
);

const externalEntries = computed(() =>
  externalLinkGroups.flatMap((group) =>
    (group.items || []).map((item) => ({
      ...item,
      entryKey: `external-${group.id}-${item.id}`,
      tags: [...(item.tags || []), group.title].filter(Boolean)
    }))
  )
);

const allEntries = computed(() => [...internalEntries.value, ...externalEntries.value]);

const filteredEntries = computed(() => {
  const query = normalizedQuery.value;
  if (!query) return allEntries.value;
  return allEntries.value.filter((item) => matchesQuery(item, query));
});

onMounted(() => {
  if (!authStore.user) {
    authStore.loadProfile();
  }
});
</script>
