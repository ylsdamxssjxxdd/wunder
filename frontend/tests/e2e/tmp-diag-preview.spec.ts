import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

// Temporary diagnostic: measure rendered image geometry in the chat-bubble preview.
test('diagnose chat image preview aspect ratio', async ({ page }) => {
  test.setTimeout(90_000);
  const service = new ChatMockService();
  for (let round = 1; round <= 3; round++) {
    const turn_id = `diag-turn-${round}`;
    service.turns.push({ turn_id, user_round: round, status: 'completed' });
    service.items.set(`${turn_id}:user`, { item_id: `${turn_id}:user`, turn_id,
      kind: 'user_message', role: 'user', content: `Fixture request ${round}.`,
      status: 'completed', visibility: 'user', revision: 1 });
    const item_id = `${turn_id}:text-1`;
    service.items.set(item_id, { item_id, turn_id, kind: 'assistant_message', role: 'assistant',
      model_round: 1, status: 'completed', revision: 2, visibility: 'user',
      content: round === 1 ? 'no image this round' : `![Fixture image ${round}](fixture-${round}.png)` });
  }
  await service.install(page);
  await page.route('**/workspace/download?**', async route => {
    await route.fulfill({ contentType: 'image/png', body: readFileSync('tests/e2e/fixtures/preview-tall.png') });
  });
  try {
    await page.setViewportSize({ width: 1600, height: 1200 });
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    await expect(page.locator('.ai-resource-card[data-workspace-state="ready"]')).toHaveCount(2);
    await page.locator('.ai-resource-card img').first().click();

    const preview = page.locator('.messenger-image-preview-dialog .zoomable-image');
    await expect(preview).toBeVisible();
    await page.waitForTimeout(1500);

    const scopeCheck = await page.evaluate(() => {
      const dialog = document.querySelector('.messenger-image-preview-dialog') as HTMLElement | null;
      if (!dialog) return { found: false };
      // Walk ancestors looking for a scoped-style attribute (data-v-*).
      const chain: string[] = [];
      let node: HTMLElement | null = dialog;
      let scopedAncestor: string | null = null;
      while (node) {
        const attrs = Array.from(node.attributes || []).map(a => a.name);
        const scoped = attrs.find(a => a.startsWith('data-v-'));
        chain.push(`${node.tagName.toLowerCase()}.${(node.className || '').toString().split(' ')[0]}${scoped ? '[' + scoped + ']' : ''}`);
        if (scoped && !scopedAncestor) scopedAncestor = node.tagName.toLowerCase() + '.' + (node.className || '').toString().split(' ')[0];
        node = node.parentElement;
      }
      const body = document.querySelector('.messenger-image-preview-body') as HTMLElement | null;
      return {
        found: true,
        dialogHasScopedAttr: Array.from(dialog.attributes).some(a => a.name.startsWith('data-v-')),
        bodyHasScopedAttr: body ? Array.from(body.attributes).some(a => a.name.startsWith('data-v-')) : null,
        scopedAncestorOfDialog: scopedAncestor,
        ancestorChain: chain.slice(0, 6)
      };
    });
    console.log('SCOPE_CHECK ' + JSON.stringify(scopeCheck, null, 2));

    const info = await preview.evaluate((image) => {
      const el = image as HTMLImageElement;
      const rect = el.getBoundingClientRect();
      const box = (selector: string) => {
        const node = el.closest(selector) as HTMLElement | null;
        if (!node) return null;
        const r = node.getBoundingClientRect();
        const cs = getComputedStyle(node);
        return { size: [Math.round(r.width), Math.round(r.height)],
          height: cs.height, minHeight: cs.minHeight, maxHeight: cs.maxHeight,
          flex: cs.flex, overflow: cs.overflow, display: cs.display };
      };
      return {
        natural: [el.naturalWidth, el.naturalHeight],
        naturalRatio: +(el.naturalWidth / el.naturalHeight).toFixed(3),
        rendered: [Math.round(rect.width), Math.round(rect.height)],
        renderedRatio: +(rect.width / rect.height).toFixed(3),
        inlineStyle: el.getAttribute('style'),
        dialog: box('.messenger-image-preview-dialog'),
        dialogBody: box('.el-dialog__body'),
        previewBody: box('.messenger-image-preview-body'),
        preview: box('.zoomable-image-preview'),
        surface: box('.zoomable-image-surface'),
        stage: box('.zoomable-image-stage')
      };
    });
    console.log('CHAT_PREVIEW ' + JSON.stringify(info, null, 2));
  } finally {
    await service.dispose();
  }
});
