import { renderChatInWorker } from '@/components/chat/chatRenderWorker';
import { renderMarkdown } from '@/utils/markdown';

export const runChatWorkerProbe = async () => {
  const source = '# Heading\n\nInline $x^2$\n\n```js\nconst value = 1;\n```\n\n[file](./sample.txt)\n\n| a | b |\n| - | - |\n| 1 | 2 |';
  const resolvePath = (path: string) => path === './sample.txt' ? '/workspaces/user-1/sample.txt' : '';
  const expected = renderMarkdown(source, { resolveWorkspacePath: resolvePath });
  const rendered = await renderChatInWorker('markdown', source, new AbortController().signal, resolvePath);
  if (!rendered?.html || rendered.html !== expected) throw new Error('Worker changed Markdown/resource rendering');
  let frames = 0;
  let sampling = true;
  const tick = () => { frames++; if (sampling) requestAnimationFrame(tick); };
  requestAnimationFrame(tick);
  const detail = JSON.stringify({ rows: Array.from({ length: 20000 }, (_, index) => ({ index, text: 'x'.repeat(80) })) });
  const result = await renderChatInWorker('detail', detail, new AbortController().signal);
  sampling = false;
  if (!result?.text || !result.parsed) throw new Error('Worker detail parse did not complete');
  const abort = new AbortController();
  const cancelled = renderChatInWorker('markdown', source.repeat(100), abort.signal);
  abort.abort();
  if (await cancelled !== null) throw new Error('Cancelled worker job was committed');
  const next = await renderChatInWorker('markdown', source, new AbortController().signal, resolvePath);
  if (next?.html !== expected) throw new Error('Worker did not recover after cancellation');
  return { frames, detailChars: detail.length, previewChars: result.text.length,
    clonedChars: JSON.stringify(result.parsed).length };
};
