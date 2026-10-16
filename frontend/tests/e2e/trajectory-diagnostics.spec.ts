import { expect, test } from '@playwright/test';
import { resolve } from 'node:path';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

const failure = { status: 'failed', code: 'UPSTREAM_TIMEOUT', message: 'fixture provider timeout',
  source_stage: 'llm', retryable: true, retry_after_ms: 500, recovery_action: 'retry_later' };
const retry = { attempt: 1, max_attempts: 3, delay_s: 0.2, message: 'fixture transient failure', will_retry: false };
const rows = [
  { item_id: 'execution:text-1', turn_id: 'execution', root_turn_id: 'execution', user_round: 1,
    created_seq: 1, item_index: 0, kind: 'assistant_message', status: 'failed', summary: 'fixture partial response',
    started_at: 1800000000, completed_at: 1800000002, model_round: 1, diagnostics: { failure, retry },
    detail: { body_id: 'execution:text-1|detail', revision: 1, bytes: 32 } },
  { item_id: 'execution:text-2', turn_id: 'execution', root_turn_id: 'execution', user_round: 1,
    created_seq: 2, item_index: 1, kind: 'assistant_message', status: 'completed', summary: 'fixture successful response',
    started_at: 1800000003, completed_at: 1800000005, model_round: 2, diagnostics: { retry } },
];

async function inspect(page: import('@playwright/test').Page) {
  await expect(page.locator('.tt-row')).toHaveCount(2);
  await expect(page.locator('.tt-row-error')).toHaveCount(1);
  await page.locator('.tt-row').first().click();
  await expect(page.locator('.tt-details')).toContainText('UPSTREAM_TIMEOUT');
  await expect(page.locator('.tt-details')).toContainText('fixture provider timeout');
  await expect(page.locator('.tt-details')).toContainText('fixture transient failure');
  await page.locator('.tt-row').last().click();
  await expect(page.locator('.tt-details')).toContainText('fixture transient failure');
  await expect(page.locator('.tt-details')).not.toContainText('UPSTREAM_TIMEOUT');
}

test('hive paged trajectory exposes final failure and successful retry details', async ({ page }) => {
  const service = new ChatMockService();
  await service.install(page);
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.route('**/view/bootstrap**', route => route.fulfill({ json: { data: { page: {
      session_id: MOCK_SESSION, epoch: 1, view_seq: 10, projection_state: 'ready',
      latest_round: 1, runtime_status: 'idle', trajectory: rows, tiles: [], has_more: false,
    } } } }));
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/trajectory?session=${MOCK_SESSION}`);
    await inspect(page);
    expect(errors).toEqual([]);
  } finally { await service.dispose(); }
});

test('bridge inspector matches hive failure and retry semantics', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto('/');
  const webRoot = resolve(process.cwd(), '../web').replaceAll('\\', '/');
  await page.evaluate(async ({ data, root }) => {
    const mount = document.createElement('div');
    mount.style.cssText = 'position:fixed;inset:0;z-index:99999;background:white';
    document.body.appendChild(mount);
    const style = document.createElement('link');
    style.rel = 'stylesheet'; style.href = `/@fs/${root}/styles/trajectory.css`;
    document.head.appendChild(style);
    const moduleUrl = `/@fs/${root}/modules/trajectory-view.js`;
    const { createTrajectoryView } = await import(/* @vite-ignore */ moduleUrl);
    const view = createTrajectoryView(mount);
    view.setRawTurns([{ user_turn_index: 1, items: data.map(row => ({
      ...row, payload: { content: row.summary, model_round: row.model_round, diagnostics: row.diagnostics },
      created_time: row.started_at, updated_time: row.completed_at,
    })) }]);
  }, { data: rows, root: webRoot });
  await inspect(page);
  expect(errors).toEqual([]);
});
