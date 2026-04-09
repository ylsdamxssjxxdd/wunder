<template>
  <div class="beeroom-chat-markdown messenger-markdown">
    <div
      v-if="renderedHtml"
      ref="contentRef"
      class="markdown-body"
      @click="handleContentClick"
      v-html="renderedHtml"
    ></div>
    <span v-else class="markdown-body beeroom-chat-markdown-empty">{{ normalizedContent }}</span>
  </div>

  <MessengerImagePreviewDialog
    :visible="imagePreviewVisible"
    :image-url="imagePreviewUrl"
    :title="imagePreviewTitle"
    :workspace-path="imagePreviewWorkspacePath"
    @download="handleImagePreviewDownload"
    @close="closeImagePreview"
  />
</template>

<script setup lang="ts">
import { ElMessage } from 'element-plus';
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';

import MessengerImagePreviewDialog from '@/components/messenger/MessengerImagePreviewDialog.vue';
import { useI18n } from '@/i18n';
import { copyText } from '@/utils/clipboard';
import { renderMarkdown, hydrateExternalMarkdownImages } from '@/utils/markdown';
import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';
import { parseWorkspaceResourceUrl } from '@/utils/workspaceResources';

const props = defineProps<{
  cacheKey: string;
  content: string;
}>();

type MarkdownCacheEntry = {
  source: string;
  html: string;
};

const MARKDOWN_CACHE_LIMIT = 160;
const markdownCache = new Map<string, MarkdownCacheEntry>();

const { t } = useI18n();
const contentRef = ref<HTMLElement | null>(null);
const imagePreviewVisible = ref(false);
const imagePreviewUrl = ref('');
const imagePreviewTitle = ref('');
const imagePreviewWorkspacePath = ref('');
let hydrationFrame: number | null = null;

const trimMarkdownCache = () => {
  while (markdownCache.size > MARKDOWN_CACHE_LIMIT) {
    const oldestKey = markdownCache.keys().next().value;
    if (!oldestKey) break;
    markdownCache.delete(oldestKey);
  }
};

const normalizedContent = computed(() => prepareMessageMarkdownContent(props.content, null));

const renderedHtml = computed(() => {
  const source = String(normalizedContent.value || '').trim();
  const cacheKey = String(props.cacheKey || '').trim();
  if (!source) {
    if (cacheKey) {
      markdownCache.delete(cacheKey);
    }
    return '';
  }
  if (!cacheKey) {
    return renderMarkdown(source);
  }
  const cached = markdownCache.get(cacheKey);
  if (cached && cached.source === source) {
    return cached.html;
  }
  const html = renderMarkdown(source);
  markdownCache.set(cacheKey, { source, html });
  trimMarkdownCache();
  return html;
});

const scheduleHydration = () => {
  if (typeof window === 'undefined') return;
  if (hydrationFrame !== null) {
    window.cancelAnimationFrame(hydrationFrame);
  }
  void nextTick(() => {
    hydrationFrame = window.requestAnimationFrame(() => {
      hydrationFrame = null;
      const container = contentRef.value;
      if (!container) return;
      const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
      cards.forEach((node) => {
        const card = node as HTMLElement;
        const kind = String(card.dataset.workspaceKind || 'image').trim().toLowerCase();
        if (kind !== 'image') {
          card.dataset.workspaceState = 'ready';
          card.classList.add('is-ready');
          return;
        }
        const publicPath = String(card.dataset.workspacePath || '').trim();
        const preview = card.querySelector('.ai-resource-preview') as HTMLImageElement | null;
        const status = card.querySelector('.ai-resource-status') as HTMLElement | null;
        if (!publicPath || !preview) return;
        if (preview.getAttribute('src') === publicPath && preview.complete && preview.naturalWidth > 0) {
          card.dataset.workspaceState = 'ready';
          card.classList.add('is-ready');
          if (status) status.textContent = '';
          return;
        }
        card.dataset.workspaceState = 'loading';
        card.classList.remove('is-error');
        card.classList.remove('is-ready');
        if (status) status.textContent = t('chat.resourceImageLoading');
        preview.onload = () => {
          card.dataset.workspaceState = 'ready';
          card.classList.remove('is-error');
          card.classList.add('is-ready');
          if (status) status.textContent = '';
        };
        preview.onerror = () => {
          card.dataset.workspaceState = 'error';
          card.classList.remove('is-ready');
          card.classList.add('is-error');
          if (status) status.textContent = t('chat.resourceImageFailed');
        };
        preview.src = publicPath;
      });
      hydrateExternalMarkdownImages(container);
    });
  });
};

const downloadWorkspaceResource = (publicPath: string) => {
  const normalized = String(publicPath || '').trim();
  if (!normalized || typeof document === 'undefined') return;
  const filename = String(parseWorkspaceResourceUrl(normalized)?.filename || 'download').trim() || 'download';
  const link = document.createElement('a');
  link.href = normalized;
  link.download = filename;
  link.rel = 'noopener';
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
};

const openImagePreview = (src: string, title: string, workspacePath: string) => {
  const normalizedSrc = String(src || '').trim();
  if (!normalizedSrc) return;
  imagePreviewUrl.value = normalizedSrc;
  imagePreviewTitle.value = String(title || '').trim() || t('chat.imagePreview');
  imagePreviewWorkspacePath.value = String(workspacePath || '').trim();
  imagePreviewVisible.value = true;
};

const closeImagePreview = () => {
  imagePreviewVisible.value = false;
  imagePreviewUrl.value = '';
  imagePreviewTitle.value = '';
  imagePreviewWorkspacePath.value = '';
};

const handleImagePreviewDownload = () => {
  const workspacePath = String(imagePreviewWorkspacePath.value || '').trim();
  if (!workspacePath) return;
  downloadWorkspaceResource(workspacePath);
};

const handleContentClick = async (event: MouseEvent) => {
  const target = event.target as HTMLElement | null;
  if (!target) return;

  const previewImage = target.closest('img.ai-resource-preview') as HTMLImageElement | null;
  if (previewImage) {
    const card = previewImage.closest('.ai-resource-card') as HTMLElement | null;
    const src = String(previewImage.getAttribute('src') || '').trim();
    const title = String(card?.querySelector('.ai-resource-name')?.textContent || '').trim();
    const workspacePath = String(card?.dataset?.workspacePath || '').trim();
    if (!src) return;
    event.preventDefault();
    openImagePreview(src, title, workspacePath);
    return;
  }

  const resourceButton = target.closest('[data-workspace-action]') as HTMLElement | null;
  if (resourceButton) {
    const container = resourceButton.closest('[data-workspace-path]') as HTMLElement | null;
    const publicPath = String(container?.dataset?.workspacePath || '').trim();
    if (!publicPath) return;
    event.preventDefault();
    downloadWorkspaceResource(publicPath);
    return;
  }

  const resourceLink = target.closest('a.ai-resource-link[data-workspace-path]') as HTMLElement | null;
  if (resourceLink) {
    const publicPath = String(resourceLink.dataset?.workspacePath || '').trim();
    if (!publicPath) return;
    event.preventDefault();
    downloadWorkspaceResource(publicPath);
    return;
  }

  const copyButton = target.closest('.ai-code-copy') as HTMLElement | null;
  if (!copyButton) return;
  event.preventDefault();
  const codeBlock = copyButton.closest('.ai-code-block');
  const codeText = String(codeBlock?.querySelector('code')?.textContent || '').trim();
  if (!codeText) {
    ElMessage.warning(t('chat.message.copyEmpty'));
    return;
  }
  const copied = await copyText(codeText);
  if (copied) {
    ElMessage.success(t('chat.message.copySuccess'));
  } else {
    ElMessage.warning(t('chat.message.copyFailed'));
  }
};

watch(
  () => renderedHtml.value,
  () => {
    if (!renderedHtml.value) return;
    scheduleHydration();
  },
  { immediate: true }
);

onBeforeUnmount(() => {
  if (hydrationFrame !== null && typeof window !== 'undefined') {
    window.cancelAnimationFrame(hydrationFrame);
  }
});
</script>

<style scoped>
.beeroom-chat-markdown {
  width: 100%;
  min-width: 0;
}

.beeroom-chat-markdown-empty {
  white-space: pre-wrap;
  word-break: break-word;
}

.beeroom-chat-markdown :deep(.markdown-body) {
  color: inherit;
}

.beeroom-chat-markdown :deep(p),
.beeroom-chat-markdown :deep(ul),
.beeroom-chat-markdown :deep(ol),
.beeroom-chat-markdown :deep(pre),
.beeroom-chat-markdown :deep(blockquote),
.beeroom-chat-markdown :deep(.ai-rich-table),
.beeroom-chat-markdown :deep(.ai-resource-card) {
  margin: 0;
}

.beeroom-chat-markdown :deep(* + p),
.beeroom-chat-markdown :deep(* + ul),
.beeroom-chat-markdown :deep(* + ol),
.beeroom-chat-markdown :deep(* + pre),
.beeroom-chat-markdown :deep(* + blockquote),
.beeroom-chat-markdown :deep(* + .ai-rich-table),
.beeroom-chat-markdown :deep(* + .ai-resource-card) {
  margin-top: 8px;
}

.beeroom-chat-markdown :deep(.markdown-body) {
  font-size: 12.5px;
  line-height: 1.65;
  word-break: break-word;
}

.beeroom-chat-markdown :deep(.ai-code-block) {
  border-radius: 12px;
}

.beeroom-chat-markdown :deep(.ai-code-header) {
  padding: 6px 8px;
}

.beeroom-chat-markdown :deep(.ai-code-block pre) {
  max-height: 240px;
  padding: 10px 12px;
}

.beeroom-chat-markdown :deep(.ai-rich-table) {
  max-width: 100%;
  overflow-x: auto;
}

.beeroom-chat-markdown :deep(.ai-rich-table table) {
  min-width: 420px;
}

.beeroom-chat-markdown :deep(.ai-resource-card) {
  max-width: min(100%, 232px);
  border-radius: 12px;
}

.beeroom-chat-markdown :deep(.ai-resource-card.ai-resource-image .ai-resource-header) {
  padding: 8px 9px 0;
}

.beeroom-chat-markdown :deep(.ai-resource-card.ai-resource-image .ai-resource-body) {
  min-height: 88px;
  padding: 8px 9px 10px;
}

.beeroom-chat-markdown :deep(.ai-resource-preview) {
  max-height: 132px;
  border-radius: 10px;
}

.beeroom-chat-markdown :deep(.ai-resource-file-header) {
  padding: 8px 9px 0;
}

.beeroom-chat-markdown :deep(.ai-resource-file) {
  min-height: 54px;
  padding: 8px 9px 10px;
}

.beeroom-chat-markdown :deep(.ai-resource-file-title),
.beeroom-chat-markdown :deep(.ai-resource-name) {
  font-size: 12px;
}

.beeroom-chat-markdown :deep(.ai-resource-file-meta),
.beeroom-chat-markdown :deep(.ai-resource-meta-inline),
.beeroom-chat-markdown :deep(.ai-resource-status) {
  font-size: 10px;
}

.beeroom-chat-markdown :deep(.ai-resource-btn),
.beeroom-chat-markdown :deep(.ai-resource-file-icon) {
  transform: scale(0.94);
  transform-origin: center;
}
</style>
