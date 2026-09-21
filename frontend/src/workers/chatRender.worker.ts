import { boundToolDetailPreview } from './toolDetailPreview';
import { renderMarkdown } from '@/utils/markdownCore';
import { formatWorkflowDetailForDisplay } from '@/components/chat/toolWorkflowDetailFormatter';

// One worker owns parsing/highlighting. Only bounded text previews cross back for tool results.
self.onmessage = (event: MessageEvent) => {
  const { id, kind, source, labels, paths } = event.data;
  try {
    if (kind === 'detail') {
      let parsed: unknown = null;
      try { parsed = JSON.parse(source); } catch { /* Plain text and JSONL use the formatter. */ }
      const formatted = formatWorkflowDetailForDisplay(source);
      self.postMessage({ id, parsed: boundToolDetailPreview(parsed), text: formatted.slice(0, 24000), truncated: formatted.length > 24000 });
      return;
    }
    const unresolved = new Set<string>();
    const html = renderMarkdown(source, { labels, resolveWorkspacePath: raw => {
      if (!paths) unresolved.add(raw);
      return paths?.[raw] || '';
    } });
    // Resolver closures stay on the main thread; exchange just resource references.
    if (!paths && unresolved.size) self.postMessage({ id, paths: [...unresolved] });
    else self.postMessage({ id, html });
  } catch {
    self.postMessage({ id, error: true });
  }
};
