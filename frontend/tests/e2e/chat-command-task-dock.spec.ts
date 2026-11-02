import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('background task dock expands output and terminates one command', async ({ page }) => {
  const service = new ChatMockService();
  const turn_id = 'fixture-command-turn';
  service.turns.push({ turn_id, user_round: 1, status: 'running', content: 'Fixture command' });
  service.commandTasks.push({
    command_session_id: 'fixture-command', session_id: MOCK_SESSION,
    command: 'fixture-command --wait', cwd: '/fixture', status: 'running',
    started_at: new Date(Date.now() - 5000).toISOString(), updated_at: new Date().toISOString(),
    stdout_tail: 'fixture output', stderr_tail: '', pty_tail: ''
  });
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);

    const trigger = page.getByTestId('composer-background-tasks');
    await expect(trigger).toHaveText('1');
    await trigger.hover();
    const panel = page.getByTestId('composer-background-task-panel');
    await expect(panel).toBeVisible();
    const open = panel.locator('.composer-task-row__open');
    await expect(open).toContainText('fixture-command --wait');
    await open.click();
    await expect(panel.locator('.composer-task-detail')).toContainText('fixture output');
    await panel.locator('.composer-task-stop').click();
    await expect(trigger).toHaveText('0');
    await expect(panel).toHaveCount(0);
    expect(service.requests.some(request =>
      request.method === 'DELETE' && String(request.path).endsWith('/command-sessions/fixture-command')
    )).toBe(true);
    expect(service.commandTasks[0].status).toBe('exited');
  } finally {
    await service.dispose();
  }
});
