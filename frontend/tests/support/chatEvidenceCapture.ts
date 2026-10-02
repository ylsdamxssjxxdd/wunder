import { mkdir, writeFile, copyFile } from 'node:fs/promises';
import { join } from 'node:path';
import { expect, type Page } from '@playwright/test';
import { analyzeChatEvidence, type ChatEvidence, type Row } from './chatEvidenceAnalysis';
import { redactEvidence } from './chatEvidenceRedaction';

export async function saveChatEvidence(directory: string, evidence: ChatEvidence, exports: Row[] = []) {
  await mkdir(directory, { recursive: true });
  const analysis = analyzeChatEvidence(evidence);
  // Retain prior diagnostics: rerunning successfully must not erase a long-task finding.
  const stamp = Date.now();
  for (const file of ['analysis.json', 'performance.json']) {
    await copyFile(join(directory, file), join(directory, `${stamp}-previous-${file}`)).catch(error => {
      if (error.code !== 'ENOENT') throw error;
    });
  }
  await writeFile(join(directory, 'thread-snapshot.json'), JSON.stringify(redactEvidence({ evidence_mode: evidence.mode, ...evidence.snapshot }), null, 2));
  await writeFile(join(directory, 'thread-changes.jsonl'), evidence.changes.map(row =>
    JSON.stringify(redactEvidence(row))).join('\n') + '\n');
  if (exports.length) await writeFile(join(directory, 'thread-export.jsonl'), exports.map(row =>
    JSON.stringify(redactEvidence(row))).join('\n') + '\n');
  await writeFile(join(directory, 'performance.json'), JSON.stringify(evidence.performance ?? {}, null, 2));
  await writeFile(join(directory, 'analysis.json'), JSON.stringify(analysis, null, 2));
  await writeFile(join(directory, 'analysis.md'), [
    `验收结果：${analysis.verdict}（${analysis.mode}）`, '',
    ...analysis.findings.map(item => `- ${item.severity}: ${item.code} × ${item.count}`), '',
    ...analysis.limitations, '',
    '线程内容已转换为摘要指纹；截图仅允许使用隔离测试数据。'
  ].join('\n'));
  return analysis;
}

export async function captureConversation(page: Page, directory: string, expandTools = false) {
  await mkdir(directory, { recursive: true });
  const list = page.getByTestId('messenger-message-list');
  await expect(list).toBeVisible();
  const rows = new Map<string, { key: string; turnId: string; status: string; content: string; stats: string; role: string; tools: string }>();
  const files: string[] = [];
  let top = 0;
  for (let index = 0; index < 100; index++) {
    await list.evaluate((node, offset) => { node.scrollTop = offset; node.dispatchEvent(new Event('scroll')); }, top);
    await page.waitForTimeout(120);
    if (expandTools) {
      for (const selector of ['details.message-tool-workflow:not([open]) > summary', 'details.tool-workflow-entry:not([open]) > summary']) {
        const handles = await list.locator(selector).elementHandles();
        for (const handle of handles) {
          await handle.evaluate(node => (node as HTMLElement).click());
          await page.waitForTimeout(40);
          await handle.dispose();
        }
        await page.waitForTimeout(80);
      }
    }
    const sample = await list.evaluate(node => {
      const viewport = node.getBoundingClientRect();
      return { top: node.scrollTop, height: node.clientHeight, end: node.scrollHeight - node.clientHeight,
        rows: Array.from(node.querySelectorAll<HTMLElement>('.messenger-message[data-virtual-key]'))
          .filter(item => { const rect = item.getBoundingClientRect(); return rect.bottom > viewport.top && rect.top < viewport.bottom; })
          .map(item => ({ key: item.dataset.virtualKey!, turnId: item.dataset.turnId ?? '',
            status: item.dataset.messageStatus ?? '', role: item.classList.contains('mine') ? 'user' : 'assistant',
            content: item.querySelector('.messenger-message-bubble')?.textContent?.trim() ?? '',
            tools: item.querySelector('.message-tool-workflow')?.textContent?.trim() ?? '',
            stats: item.querySelector('.messenger-message-stats')?.textContent?.trim() ?? '' })) };
    });
    for (const row of sample.rows) {
      const prior = rows.get(row.key);
      if (prior && (prior.content !== row.content || prior.role !== row.role)) throw new Error('history changed while scrolling');
      rows.set(row.key, row);
    }
    expect(new Set(sample.rows.map(row => row.key)).size).toBe(sample.rows.length);
    const file = `viewport-${String(index).padStart(3, '0')}.png`;
    // A virtual-window update can replace the scrolling node between sampling
    // and capture. Re-resolve it so evidence collection does not hide a real
    // renderer failure behind a stale Playwright handle.
    await page.getByTestId('messenger-message-list').screenshot({ path: join(directory, file) });
    files.push(file);
    if (sample.top >= sample.end - 2) break;
    if (index === 99) throw new Error('conversation screenshot page limit exceeded');
    top = Math.min(sample.end, sample.top + Math.max(1, sample.height * 0.75));
  }
  await writeFile(join(directory, 'index.html'), '<!doctype html><meta charset="utf-8"><title>Conversation review</title>' +
    '<style>body{margin:0;background:#eee}img{display:block;max-width:100%;margin:16px auto}</style>' +
    files.map(file => `<img src="${file}" loading="lazy" alt="${file}">`).join('\n'));
  await writeFile(join(directory, 'rendered-rows.json'), JSON.stringify(redactEvidence([...rows.values()]), null, 2));
  return [...rows.values()];
}
