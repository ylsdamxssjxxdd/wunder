<template>
  <div class="portal-shell external-app-shell">
    <UserTopbar :title="pageTitle" :subtitle="t('portal.external.embedSubtitle')" :hide-chat="true" />
    <main class="external-app-main">
      <div class="external-app-toolbar">
        <button class="topbar-panel-btn" type="button" @click="goHome">
          <i class="fa-solid fa-arrow-left" aria-hidden="true"></i>
          <span>{{ t('portal.external.back') }}</span>
        </button>
        <div class="external-app-meta">
          <div class="external-app-name">{{ pageTitle }}</div>
          <a
            v-if="currentLink?.url"
            :href="currentLink.url"
            target="_blank"
            rel="noopener noreferrer"
            class="external-app-origin"
          >
            {{ currentLink.url }}
          </a>
        </div>
      </div>
      <div class="external-app-frame-wrap">
        <div v-if="loading" class="agent-empty">{{ t('portal.section.loading') }}</div>
        <div v-else-if="errorMessage" class="agent-empty">{{ errorMessage }}</div>
        <iframe
          v-else-if="currentLink"
          :src="currentLink.url"
          class="external-app-frame"
          referrerpolicy="no-referrer"
        ></iframe>
      </div>
    </main>
  </div>
</template>

<script setup>
import { computed, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import { fetchExternalLinks } from '@/api/externalLinks';
import UserTopbar from '@/components/user/UserTopbar.vue';
import { useI18n } from '@/i18n';

const route = useRoute();
const router = useRouter();
const { t } = useI18n();

const loading = ref(false);
const currentLink = ref(null);
const errorMessage = ref('');

const basePath = computed(() => (route.path.startsWith('/demo') ? '/demo' : '/app'));
const pageTitle = computed(() => currentLink.value?.title || t('portal.external.embedTitle'));

const loadLink = async () => {
  const linkId = String(route.params.linkId || '').trim();
  if (!linkId) {
    currentLink.value = null;
    errorMessage.value = t('portal.external.notFound');
    return;
  }
  loading.value = true;
  errorMessage.value = '';
  try {
    const { data } = await fetchExternalLinks({ link_id: linkId });
    const items = Array.isArray(data?.data?.items) ? data.data.items : [];
    currentLink.value = items[0] || null;
    if (!currentLink.value) {
      errorMessage.value = t('portal.external.notFound');
    }
  } catch (error) {
    currentLink.value = null;
    errorMessage.value = error.response?.data?.detail || t('portal.external.loadFailed');
  } finally {
    loading.value = false;
  }
};

const goHome = () => {
  router.push(basePath.value + '/home');
};

onMounted(loadLink);

watch(
  () => route.params.linkId,
  () => {
    loadLink();
  }
);
</script>
