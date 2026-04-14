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
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';

import { downloadWunderWorkspaceFile } from '@/api/workspace';
import MessengerImagePreviewDialog from '@/components/messenger/MessengerImagePreviewDialog.vue';
import { useI18n } from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { copyText } from '@/utils/clipboard';
import { renderMarkdown, hydrateExternalMarkdownImages } from '@/utils/markdown';
import { prepareMessageMarkdownContent } from '@/utils/messageMarkdown';
import { normalizeWorkspaceOwnerId } from '@/utils/messageWorkspacePath';
import {
  clearWorkspaceLoadingLabelTimer,
  getFilenameFromHeaders,
  normalizeWorkspaceImageBlob,
  resetWorkspaceImageCards,
  saveObjectUrlAsFile,
  scheduleWorkspaceLoadingLabel
} from '@/utils/workspaceResourceCards';
import { parseWorkspaceResourceUrl } from '@/utils/workspaceResources';

const props = defineProps<{
  cacheKey: string;
  content: string;
}>();

type MarkdownCacheEntry = {
  source: string;
  html: string;
};

type WorkspaceResolvedResource = NonNullable<ReturnType<typeof parseWorkspaceResourceUrl>> & {
  requestUserId: string | null;
  requestAgentId: string | null;
  requestContainerId: number | null;
  allowed: boolean;
};

type WorkspaceResourceCacheEntry = {
  objectUrl?: string;
  filename?: string;
  promise?: Promise<{ objectUrl: string; filename: string }>;
};

const MARKDOWN_CACHE_LIMIT = 160;
const markdownCache = new Map<string, MarkdownCacheEntry>();
const workspaceResourceCache = new Map<string, WorkspaceResourceCacheEntry>();

const { t } = useI18n();
const authStore = useAuthStore();
const contentRef = ref<HTMLElement | null>(null);
const imagePreviewVisible = ref(false);
const imagePreviewUrl = ref('');
const imagePreviewTitle = ref('');
const imagePreviewWorkspacePath = ref('');
let hydrationFrame: number | null = null;

const isAdminUser = (user: Record<string, unknown> | null): boolean =>
  Array.isArray(user?.roles) &&
  user.roles.some((role) => role === 'admin' || role === 'super_admin');

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

const resolveWorkspaceResource = (publicPath: string): WorkspaceResolvedResource | null => {
  const parsed = parseWorkspaceResourceUrl(publicPath);
  if (!parsed) return null;
  const user = authStore.user as Record<string, unknown> | null;
  if (!user) return null;
  const currentId = normalizeWorkspaceOwnerId(user.id);
  const workspaceId = parsed.workspaceId || parsed.userId;
  const ownerId = parsed.ownerId || workspaceId;
  const agentId = parsed.agentId || '';
  const containerId =
    typeof parsed.containerId === 'number' && Number.isFinite(parsed.containerId)
      ? parsed.containerId
      : null;
  const isOwner =
    Boolean(currentId) &&
    (workspaceId === currentId ||
      workspaceId.startsWith(`${currentId}__agent__`) ||
      workspaceId.startsWith(`${currentId}__a__`) ||
      workspaceId.startsWith(`${currentId}__c__`));
  if (isOwner) {
    return {
      ...parsed,
      requestUserId: null,
      requestAgentId: agentId || null,
      requestContainerId: containerId,
      allowed: true
    };
  }
  if (isAdminUser(user)) {
    return {
      ...parsed,
      requestUserId: ownerId,
      requestAgentId: agentId || null,
      requestContainerId: containerId,
      allowed: true
    };
  }
  // Prefer current login context for non-admin requests to avoid cross-display ID mismatches.
  return {
    ...parsed,
    requestUserId: null,
    requestAgentId: agentId || null,
    requestContainerId: containerId,
    allowed: true
  };
};

const fetchWorkspaceResource = async (resource: WorkspaceResolvedResource) => {
  const cacheKey = resource.publicPath;
  const cached = workspaceResourceCache.get(cacheKey);
  if (cached?.objectUrl) {
    return {
      objectUrl: cached.objectUrl,
      filename: cached.filename || resource.filename || 'download'
    };
  }
  if (cached?.promise) return cached.promise;
  const promise = (async () => {
    const params: Record<string, string> = {
      path: String(resource.relativePath || '')
    };
    if (resource.requestUserId) {
      params.user_id = resource.requestUserId;
    }
    if (resource.requestAgentId) {
      params.agent_id = resource.requestAgentId;
    }
    if (resource.requestContainerId !== null && Number.isFinite(resource.requestContainerId)) {
      params.container_id = String(resource.requestContainerId);
    }
    const response = await downloadWunderWorkspaceFile(params);
    try {
      const filename = getFilenameFromHeaders(
        response?.headers as Record<string, unknown>,
        resource.filename || 'download'
      );
      const contentType = String(
        (response?.headers as Record<string, unknown>)?.['content-type'] ||
          (response?.headers as Record<string, unknown>)?.['Content-Type'] ||
          ''
      );
      const blob = normalizeWorkspaceImageBlob(response.data as Blob, filename, contentType);
      const objectUrl = URL.createObjectURL(blob);
      const entry = { objectUrl, filename };
      workspaceResourceCache.set(cacheKey, entry);
      return entry;
    } catch (error) {
      workspaceResourceCache.delete(cacheKey);
      throw error;
    }
  })().catch((error) => {
    workspaceResourceCache.delete(cacheKey);
    throw error;
  });
  workspaceResourceCache.set(cacheKey, { promise });
  return promise;
};

const isWorkspaceResourceMissing = (error: unknown): boolean => {
  const status = Number((error as { response?: { status?: unknown } })?.response?.status || 0);
  if (status === 404 || status === 410) return true;
  const raw =
    (error as { response?: { data?: { detail?: string; message?: string } } })?.response?.data?.detail ||
    (error as { response?: { data?: { message?: string } } })?.response?.data?.message ||
    (error as { message?: string })?.message ||
    '';
  const message = typeof raw === 'string' ? raw : String(raw || '');
  return /not found|no such|不存在|找不到|已删除|已移除|removed/i.test(message);
};

const hydrateWorkspaceResourceCard = async (card: HTMLElement) => {
  if (!card || card.dataset.workspaceState) return;
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
  const resource = resolveWorkspaceResource(publicPath);
  if (!resource) {
    if (status) status.textContent = t('chat.resourceUnavailable');
    card.dataset.workspaceState = 'error';
    card.classList.add('is-error');
    return;
  }
  if (!resource.allowed) {
    if (status) status.textContent = t('chat.resourceDenied');
    card.dataset.workspaceState = 'forbidden';
    card.classList.add('is-error');
    return;
  }
  card.dataset.workspaceState = 'loading';
  card.classList.remove('is-error');
  card.classList.remove('is-ready');
  const loadingTimerId = scheduleWorkspaceLoadingLabel(card, status, t('chat.resourceImageLoading'));
  const markReady = () => {
    clearWorkspaceLoadingLabelTimer(loadingTimerId);
    card.dataset.workspaceState = 'ready';
    card.classList.remove('is-error');
    card.classList.add('is-ready');
    if (status) status.textContent = '';
  };
  const markError = (message: string) => {
    clearWorkspaceLoadingLabelTimer(loadingTimerId);
    card.dataset.workspaceState = 'error';
    card.classList.remove('is-ready');
    card.classList.add('is-error');
    if (status) status.textContent = message;
  };
  try {
    const entry = await fetchWorkspaceResource(resource);
    preview.onload = () => markReady();
    preview.onerror = () => markError(t('chat.resourceImageFailed'));
    preview.src = entry.objectUrl;
    if (preview.complete) {
      if (preview.naturalWidth > 0) {
        markReady();
      } else {
        markError(t('chat.resourceImageFailed'));
      }
    }
  } catch (error) {
    markError(isWorkspaceResourceMissing(error) ? t('chat.resourceMissing') : t('chat.resourceImageFailed'));
  }
};

const resetWorkspaceResourceCards = () => {
  resetWorkspaceImageCards(contentRef.value, { clearSrc: true });
};

const scheduleHydration = (options: { resetStale?: boolean } = {}) => {
  if (typeof window === 'undefined') return;
  if (hydrationFrame !== null) {
    window.cancelAnimationFrame(hydrationFrame);
  }
  void nextTick(() => {
    hydrationFrame = window.requestAnimationFrame(() => {
      hydrationFrame = null;
      const container = contentRef.value;
      if (!container) return;
      if (options.resetStale) {
        resetWorkspaceResourceCards();
      }
      const cards = container.querySelectorAll('.ai-resource-card[data-workspace-path]');
      cards.forEach((node) => {
        const card = node as HTMLElement;
        void hydrateWorkspaceResourceCard(card);
      });
      hydrateExternalMarkdownImages(container);
    });
  });
};

const clearWorkspaceResourceCache = () => {
  workspaceResourceCache.forEach((entry) => {
    if (entry?.objectUrl) {
      URL.revokeObjectURL(entry.objectUrl);
    }
  });
  workspaceResourceCache.clear();
};

const downloadWorkspaceResource = async (publicPath: string) => {
  const resource = resolveWorkspaceResource(publicPath);
  if (!resource) return;
  if (!resource.allowed) {
    ElMessage.warning(t('chat.resourceDenied'));
    return;
  }
  try {
    const entry = await fetchWorkspaceResource(resource);
    saveObjectUrlAsFile(entry.objectUrl, entry.filename || resource.filename || 'download');
  } catch (error) {
    ElMessage.error(
      isWorkspaceResourceMissing(error) ? t('chat.resourceMissing') : t('chat.resourceDownloadFailed')
    );
  }
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

const handleImagePreviewDownload = async () => {
  const workspacePath = String(imagePreviewWorkspacePath.value || '').trim();
  if (!workspacePath) return;
  await downloadWorkspaceResource(workspacePath);
};

const handleContentClick = async (event: MouseEvent) => {
  const target = event.target as HTMLElement | null;
  if (!target) return;

  const previewImage = target.closest('img.ai-resource-preview') as HTMLImageElement | null;
  if (previewImage) {
    const card = previewImage.closest('.ai-resource-card') as HTMLElement | null;
    if (card?.dataset?.workspaceState !== 'ready') return;
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
    await downloadWorkspaceResource(publicPath);
    return;
  }

  const resourceLink = target.closest('a.ai-resource-link[data-workspace-path]') as HTMLElement | null;
  if (resourceLink) {
    const publicPath = String(resourceLink.dataset?.workspacePath || '').trim();
    if (!publicPath) return;
    event.preventDefault();
    await downloadWorkspaceResource(publicPath);
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
    scheduleHydration({ resetStale: true });
  },
  { immediate: true }
);

onMounted(() => {
  if (!renderedHtml.value) return;
  scheduleHydration({ resetStale: true });
});

onBeforeUnmount(() => {
  if (hydrationFrame !== null && typeof window !== 'undefined') {
    window.cancelAnimationFrame(hydrationFrame);
  }
  clearWorkspaceResourceCache();
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
