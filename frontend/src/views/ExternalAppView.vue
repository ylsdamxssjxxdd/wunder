<template>
  <div class="external-app-page">
    <iframe
      v-if="currentLink && !errorMessage"
      :src="currentLink.url"
      class="external-app-frame"
      referrerpolicy="no-referrer"
    ></iframe>
    <div v-if="loading" class="external-app-overlay">{{ t('portal.section.loading') }}</div>
    <div v-else-if="errorMessage" class="external-app-overlay is-error">{{ errorMessage }}</div>
    <button
      class="external-world-fab"
      :class="{ 'is-dragging': isDraggingFab }"
      type="button"
      :title="t('portal.external.back')"
      :aria-label="t('portal.external.back')"
      :style="floatingButtonStyle"
      @pointerdown="handleFabPointerDown"
      @click="handleFabClick"
    >
      <i class="fa-solid fa-earth-asia" aria-hidden="true"></i>
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';

import { fetchExternalLinks } from '@/api/externalLinks';
import { useI18n } from '@/i18n';

const route = useRoute();
const router = useRouter();
const { t } = useI18n();

const FAB_SIZE = 48;
const FAB_MARGIN = 12;
const DRAG_THRESHOLD = 6;

const loading = ref(false);
const currentLink = ref(null);
const errorMessage = ref('');
const isDraggingFab = ref(false);
const suppressFabClick = ref(false);
const fabPosition = ref({ x: 16, y: 16 });

let dragPointerId = null;
let dragStartClientX = 0;
let dragStartClientY = 0;
let dragStartFabX = 0;
let dragStartFabY = 0;

const basePath = computed(() => (route.path.startsWith('/demo') ? '/demo' : '/app'));
const floatingButtonStyle = computed(() => ({
  left: `${fabPosition.value.x}px`,
  top: `${fabPosition.value.y}px`
}));

const clampFabPosition = (x, y) => {
  const maxX = Math.max(FAB_MARGIN, window.innerWidth - FAB_SIZE - FAB_MARGIN);
  const maxY = Math.max(FAB_MARGIN, window.innerHeight - FAB_SIZE - FAB_MARGIN);
  return {
    x: Math.min(Math.max(x, FAB_MARGIN), maxX),
    y: Math.min(Math.max(y, FAB_MARGIN), maxY)
  };
};

const setFabPosition = (x, y) => {
  fabPosition.value = clampFabPosition(x, y);
};

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

const handleFabPointerDown = (event) => {
  if (typeof event.button === 'number' && event.button !== 0) {
    return;
  }
  event.preventDefault();
  isDraggingFab.value = true;
  suppressFabClick.value = false;
  dragPointerId = event.pointerId;
  dragStartClientX = event.clientX;
  dragStartClientY = event.clientY;
  dragStartFabX = fabPosition.value.x;
  dragStartFabY = fabPosition.value.y;
};

const handleFabPointerMove = (event) => {
  if (!isDraggingFab.value || dragPointerId !== event.pointerId) {
    return;
  }
  event.preventDefault();
  const deltaX = event.clientX - dragStartClientX;
  const deltaY = event.clientY - dragStartClientY;
  if (Math.abs(deltaX) > DRAG_THRESHOLD || Math.abs(deltaY) > DRAG_THRESHOLD) {
    suppressFabClick.value = true;
  }
  setFabPosition(dragStartFabX + deltaX, dragStartFabY + deltaY);
};

const stopFabDragging = (pointerId = null) => {
  if (!isDraggingFab.value) {
    return;
  }
  if (pointerId !== null && dragPointerId !== pointerId) {
    return;
  }
  isDraggingFab.value = false;
  dragPointerId = null;
};

const handleFabPointerUp = (event) => {
  stopFabDragging(event.pointerId);
};

const handleFabClick = () => {
  if (suppressFabClick.value) {
    suppressFabClick.value = false;
    return;
  }
  goHome();
};

const handleWindowResize = () => {
  setFabPosition(fabPosition.value.x, fabPosition.value.y);
};

onMounted(() => {
  loadLink();
  window.addEventListener('pointermove', handleFabPointerMove, { passive: false });
  window.addEventListener('pointerup', handleFabPointerUp);
  window.addEventListener('pointercancel', handleFabPointerUp);
  window.addEventListener('resize', handleWindowResize);
  handleWindowResize();
});

onBeforeUnmount(() => {
  window.removeEventListener('pointermove', handleFabPointerMove);
  window.removeEventListener('pointerup', handleFabPointerUp);
  window.removeEventListener('pointercancel', handleFabPointerUp);
  window.removeEventListener('resize', handleWindowResize);
});

watch(
  () => route.params.linkId,
  () => {
    loadLink();
  }
);
</script>
