import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

type ImageGeometry = {
  natural: [number, number];
  naturalRatio: number;
  rendered: [number, number];
  renderedRatio: number;
  stage: [number, number];
};

/**
 * The chat-bubble image preview dialog uses el-dialog `append-to-body`, so it is
 * teleported out of the component tree and component scoped `:deep()` styles
 * never apply to it. When the image stage therefore keeps its intrinsic
 * min-height instead of filling the dialog body, fit-scale is derived from a
 * collapsed box and the image is drawn far smaller than the space available.
 * These assertions pin the stage to the dialog body and the rendered box to the
 * intrinsic aspect ratio for both preview entry points.
 */
const measure = async (image: import('@playwright/test').Locator): Promise<ImageGeometry> =>
  image.evaluate((node) => {
    const el = node as HTMLImageElement;
    const rect = el.getBoundingClientRect();
    const stage = el.closest('.zoomable-image-stage') as HTMLElement | null;
    const stageRect = stage?.getBoundingClientRect();
    return {
      natural: [el.naturalWidth, el.naturalHeight],
      naturalRatio: el.naturalWidth / el.naturalHeight,
      rendered: [Math.round(rect.width), Math.round(rect.height)],
      renderedRatio: rect.width / rect.height,
      stage: stageRect ? [Math.round(stageRect.width), Math.round(stageRect.height)] : [0, 0]
    };
  });

const expectIntrinsicRatio = async (image: import('@playwright/test').Locator) => {
  const geometry = await measure(image);
  // The rendered box must never be stretched away from the intrinsic ratio.
  expect(geometry.renderedRatio).toBeCloseTo(geometry.naturalRatio, 2);
  return geometry;
};

const installChatFixture = async (page: import('@playwright/test').Page, imageFile: string) => {
  const service = new ChatMockService();
  for (let round = 1; round <= 3; round++) {
    const turn_id = `ratio-turn-${round}`;
    service.turns.push({ turn_id, user_round: round, status: 'completed' });
    service.items.set(`${turn_id}:user`, { item_id: `${turn_id}:user`, turn_id,
      kind: 'user_message', role: 'user', content: `Fixture request ${round}.`,
      status: 'completed', visibility: 'user', revision: 1 });
    const item_id = `${turn_id}:text-1`;
    const content = round === 1 ? 'No image this round.' : `![Fixture image](fixture-${round}.png)`;
    service.items.set(item_id, { item_id, turn_id, kind: 'assistant_message', role: 'assistant',
      model_round: 1, status: 'completed', revision: 2, visibility: 'user', content });
    service.blocks.set(item_id, { event: 'thread_item_block', item_id, field: 'content', block_index: 0,
      content_offset: 0, data: { item_id, turn_id, model_round: 1, field: 'content', block_index: 0,
        content_offset: 0, content } });
  }
  await service.install(page);
  await page.route('**/workspace/download?**', async route => {
    await route.fulfill({ contentType: 'image/png', body: readFileSync(imageFile) });
  });
  return service;
};

const login = async (page: import('@playwright/test').Page) => {
  await page.goto('/login');
  await page.locator('input[autocomplete="username"]').fill('fixture-user');
  await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
  await page.locator('button[type="submit"]').click();
  await page.waitForURL('**/app/**');
  await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
};

for (const [label, fixture, intrinsicRatio] of [
  ['tall', 'tests/e2e/fixtures/preview-tall.png', 1 / 3],
  ['wide', 'tests/e2e/fixtures/preview-wide.png', 3],
  ['square', 'tests/e2e/fixtures/preview-square.png', 1]
] as const) {
  test(`chat-bubble image preview keeps intrinsic aspect ratio (${label} fixture)`, async ({ page }) => {
    test.setTimeout(90_000);
    const service = await installChatFixture(page, fixture);
    try {
      await page.setViewportSize({ width: 1600, height: 1200 });
      await login(page);
      await expect(page.locator('.ai-resource-card[data-workspace-state="ready"]')).toHaveCount(2);
      await page.locator('.ai-resource-card img').first().click();

      const image = page.locator('.messenger-image-preview-dialog .zoomable-image');
      await expect(image).toBeVisible();
      await expect.poll(async () => (await measure(image)).naturalRatio).toBeCloseTo(intrinsicRatio, 2);

      const geometry = await expectIntrinsicRatio(image);

      // The stage must consume the dialog body instead of collapsing to its
      // intrinsic min-height, otherwise a tall image is shrunk to a fraction of
      // the space the dialog already reserves for it.
      const fill = await image.evaluate((node) => {
        const stage = (node as HTMLElement).closest('.zoomable-image-stage') as HTMLElement;
        const dialog = document.querySelector('.messenger-image-preview-dialog') as HTMLElement;
        return { stage: stage.clientHeight, dialog: dialog.clientHeight };
      });
      expect(fill.stage).toBeGreaterThan(fill.dialog * 0.8);
      // Fit never upscales past 100% and never overflows the stage.
      const [stageWidth, stageHeight] = geometry.stage;
      expect(geometry.rendered[0]).toBeLessThanOrEqual(stageWidth);
      expect(geometry.rendered[1]).toBeLessThanOrEqual(stageHeight);
      expect(Math.max(geometry.rendered[0], geometry.rendered[1]))
        .toBeLessThanOrEqual(Math.max(geometry.natural[0], geometry.natural[1]));

      // Reset to 100% must keep the same ratio, never stretch the pixels.
      await page.locator('.messenger-image-preview-dialog .zoomable-image-btn--label').click();
      await expectIntrinsicRatio(image);
    } finally {
      await service.dispose();
    }
  });
}
