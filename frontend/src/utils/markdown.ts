import { t } from '@/i18n';
import { isDesktopLocalModeEnabled } from '@/config/desktop';
import { renderMarkdown as renderCore, type MarkdownRenderOptions } from './markdownCore';

export const getMarkdownRenderLabels = (): Record<string, string> => ({
  'resource.download': isDesktopLocalModeEnabled() ? t('workspace.action.exportCopy') : t('common.download'),
  'workspace.preview.dialogTitle': t('workspace.preview.dialogTitle'),
  'chat.resourceImageLoading': t('chat.resourceImageLoading'),
  'chat.code.copy': t('chat.code.copy'),
  'common.copy': t('common.copy')
});

export const renderMarkdown = (content = '', options: MarkdownRenderOptions = {}) =>
  renderCore(content, { ...options, labels: getMarkdownRenderLabels() });

export function hydrateExternalMarkdownImages(container: ParentNode | null | undefined) {
  if (!container || typeof (container as ParentNode).querySelectorAll !== 'function') return;
  container.querySelectorAll('.ai-external-image-card[data-markdown-fallback]').forEach((node) => {
    const host = node as HTMLElement;
    if (host.dataset.externalImageBound === 'true') return;
    host.dataset.externalImageBound = 'true';
    const image = host.querySelector('.ai-external-image-preview') as HTMLImageElement | null;
    if (!image) return;
    const replaceWithFallback = () => {
      if (!host.isConnected) return;
      const fallbackText = String(host.dataset.markdownFallback || '').trim();
      const fallbackNode = document.createElement('span');
      fallbackNode.className = 'ai-resource-fallback';
      fallbackNode.textContent = fallbackText || String(image.getAttribute('alt') || '').trim();
      host.replaceWith(fallbackNode);
    };
    image.addEventListener('error', replaceWithFallback, { once: true });
    if (image.complete && image.naturalWidth === 0) {
      replaceWithFallback();
    }
  });
}
