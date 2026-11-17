import { expect, test } from '@playwright/test';

import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';
import { openWorkProcess } from '../support/workProcess';

test('export-shaped running reload keeps completed thoughts still and never reports a task completion',async({page})=>{
  const service=new ChatMockService();
  const turn=service.seedProcessExportFixture();
  await service.install(page);
  try{
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    for(let attempt=0;attempt<2;attempt++){
      const bubble=page.locator('.messenger-turn[data-root-turn-id="fixture-turn-1"]');
      await openWorkProcess(bubble.locator('.timeline-process-head'));
      await expect(bubble.locator('.tl-thinking')).toHaveCount(1);
      await expect(bubble.locator('.tl-thinking.is-streaming')).toHaveCount(0);
      await expect(bubble.locator('.tl-entry-head')).toHaveCount(1);
      await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode','stop');
      service.replayPrematureCompletion(turn);
      service.replayTransportFinal();
      await page.waitForTimeout(200);
      await expect(page.locator('.el-message--success')).toHaveCount(0);
      await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode','stop');
      if(attempt===0)await page.reload();
    }
  }finally{await service.dispose();}
});

test('six-round export-shaped reload restores all six tools and settles the bubble',async({page})=>{
  const service=new ChatMockService();
  service.seedProcessExportFixture(true);
  await service.install(page);
  try{
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    for(let attempt=0;attempt<2;attempt++){
      const bubble=page.locator('.messenger-turn[data-root-turn-id="fixture-turn-1"]');
      await expect(bubble).toContainText('Fixture final answer');
      await openWorkProcess(bubble.locator('.timeline-process-head'));
      await expect(bubble.locator('.tl-entry-head')).toHaveCount(6);
      await expect(bubble.locator('.tl-thinking')).toHaveCount(6);
      await expect(bubble.locator('.tl-thinking.is-streaming')).toHaveCount(0);
      await expect(page.getByTestId('chat-composer-send')).not.toHaveAttribute('data-mode','stop');
      if(attempt===0)await page.reload();
    }
  }finally{await service.dispose();}
});

test('new live reasoning is appended after durable process entries', async ({ page }) => {
  const service = new ChatMockService();
  service.seedDeferredReasoningTurn();
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);

    const bubble = page.locator('.messenger-turn[data-root-turn-id="fixture-turn-1"]');
    const process = bubble.locator('.timeline-process-head');
    await expect(process).toBeVisible();
    await openWorkProcess(process);

    const thoughts = bubble.locator('.tl-thinking');
    await expect(thoughts).toHaveCount(2);
    // 行内不再渲染摘要与省略号动画：收起的思考行只有标签与右侧指标。
    await expect(thoughts.locator('.tl-thinking-summary')).toHaveCount(0);
    await expect(thoughts.locator('.tl-thinking-ellipsis')).toHaveCount(0);
    await expect(thoughts.nth(0)).not.toContainText('Earlier completed thought.');
    await expect(thoughts.nth(0)).not.toContainText('New live thought.');
    await expect(thoughts.nth(1)).toHaveClass(/is-streaming/);
    await expect(thoughts.locator('.tl-thinking-metrics')).toHaveCount(2);
    await thoughts.nth(0).locator('.tl-thinking-head').click();
    await expect(thoughts.nth(0).locator('.tl-thinking-text')).toContainText('Earlier completed thought.');
    await expect(thoughts.nth(0).locator('.tl-thinking-text')).not.toContainText('New live thought.');
    await thoughts.nth(1).locator('.tl-thinking-head').click();
    await expect(thoughts.nth(1).locator('.tl-thinking-text')).toContainText('New live thought.');
    const metricWidths = await thoughts.locator('.tl-thinking-metrics').evaluateAll(nodes =>
      nodes.map(node => node.getBoundingClientRect().width));
    expect(new Set(metricWidths).size).toBe(1);
  } finally {
    await service.dispose();
  }
});
