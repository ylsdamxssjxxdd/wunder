import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

/**
 * 真实会话链路回归：运行中的执行命令条目自己展开（露出实时输出），命令结束
 * 自己收起。与 `timeline-command-auto-open.spec.ts` 的组件级用例互补——这里
 * 走的是线程懒加载工作过程（`MessageTimelineTurnProcess`）的完整路径。
 *
 * 该路径的行键曾多加一层 `tool:` 前缀，与自动开合按条目 key 判定的口径不一致，
 * 导致云端版运行中的命令条目不会自己展开；本用例守住「行键 = 条目 key」。
 */
test('a running execute_command entry opens itself in the live timeline and folds when it settles', async ({ page }) => {
  const service = new ChatMockService();
  const turn = service.seedRunningCommandTurn();
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);

    const bubble = page.locator(
      `.messenger-turn[data-root-turn-id="${turn.turn_id}"] [data-turn-slot="assistant"]`
    );
    const entry = bubble.locator('.tl-entry.is-command').first();
    const head = entry.locator('.tl-entry-head');

    // 命令仍在跑：条目不需要点击就铺开，实时输出直接可见。
    await expect(head).toBeVisible();
    await expect(head).toHaveAttribute('aria-expanded', 'true');
    await expect(entry.locator('.tl-entry-output')).toBeVisible();

    // 命令结束：条目自己收回。
    service.settleCommandTurn(turn);
    await expect(head).toHaveAttribute('aria-expanded', 'false');
    await expect(entry.locator('.tl-entry-output')).toHaveCount(0);

    // 读者亲手点开后，后续流式帧不再改写这一条。
    await head.click();
    await expect(head).toHaveAttribute('aria-expanded', 'true');
    service.settleCommandTurn(turn, 'Fixture output after review');
    await expect(head).toHaveAttribute('aria-expanded', 'true');
    await expect(entry.locator('.tl-entry-output')).toContainText('Fixture output after review');
  } finally {
    await service.dispose();
  }
});
