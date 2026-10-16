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

test('hive initial overview compresses idle gaps before paging and hydrates compaction output', async ({ page }) => {
  const service = new ChatMockService();
  await service.install(page);
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  const allRows = [
    { ...rows[1], item_id: 'fixture-input', created_seq: 1, kind: 'user_message',
      started_at: 1800000000, completed_at: 1800003600, summary: 'fixture input' },
    { ...rows[1], item_id: 'fixture-early', created_seq: 2,
      started_at: 1800000000, completed_at: 1800000002, summary: 'fixture early response' },
    { ...rows[1], item_id: 'fixture-late', created_seq: 3,
      started_at: 1800003600, completed_at: 1800003603, summary: 'fixture late response' },
    { ...rows[1], item_id: 'fixture-compaction', created_seq: 4, kind: 'compaction',
      started_at: 1800003603, completed_at: 1800003603, summary: 'history',
      detail: { body_id: 'fixture-compaction|detail', revision: 1, bytes: 64 } },
  ];
  const timeline = allRows.map(({ created_seq, kind, status, started_at, completed_at }) =>
    ({ created_seq, kind, status, started_at, completed_at }));
  // A very short ledger automatically seeks the older edge while mounting.
  // Hold that response until the initial-domain assertions are complete.
  let releaseOlder!: () => void;
  const olderGate = new Promise<void>(resolve => { releaseOlder = resolve; });
  const envelope = (trajectory: typeof allRows, has_more: boolean) => ({ data: { page: {
    session_id: MOCK_SESSION, epoch: 1, view_seq: 10, projection_state: 'ready',
    latest_round: 1, runtime_status: 'idle', trajectory, timeline, timeline_has_more: false,
    tiles: [{ bucket: 0, start: 1800000000, end: 1800003603, count: 4, model_count: 2, input_count: 1 }],
    tile_level: 0, has_more,
  } } });
  try {
    await page.route('**/view/bootstrap**', route => route.fulfill({ json: envelope(allRows.slice(2), true) }));
    await page.route('**/view/trajectory**', async route => {
      await olderGate;
      await route.fulfill({ json: envelope(allRows.slice(0, 2), false) });
    });
    await page.route('**/bodies/**', route => route.fulfill({ json: { data: {
      chunks: [JSON.stringify({ content: 'history', summary_text: 'Fixture retained context after compaction.' })], has_more: false,
    } } }));
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/trajectory?session=${MOCK_SESSION}`);
    await expect(page.locator('.tt-row')).toHaveCount(2);
    const geometry = async () => page.locator('.tt-track').evaluate(track => ({
      width: track.clientWidth,
      model: [...track.querySelectorAll<HTMLElement>('.tt-span-message')].map(span =>
        ({ left: parseFloat(span.style.left), width: parseFloat(span.style.width) })),
      input: [...track.querySelectorAll<HTMLElement>('.tt-span-user')].map(span => parseFloat(span.style.width)),
    }));
    const initial = await geometry();
    expect(initial.model).toHaveLength(2);
    // Five seconds of model activity share one compact domain. A 1 h idle
    // gap must never squash the first response into a near-invisible mark.
    expect(initial.model[0].width).toBeGreaterThan(initial.width * 0.35);
    expect(initial.model[1].left).toBeLessThan(initial.width * 0.45);
    expect(initial.input.every(width => width <= 2)).toBeTruthy();
    const track = await page.locator('.tt-track').boundingBox();
    expect(track).not.toBeNull();
    await page.mouse.move(track!.x + track!.width * 0.6, track!.y + 3);
    await page.mouse.down();
    await page.mouse.move(track!.x + track!.width * 0.9, track!.y + 3, { steps: 4 });
    await page.mouse.up();
    await expect(page.locator('.tt-row[data-record-id="fixture-late"]')).toHaveAttribute('data-timeline-focus', 'inside');
    await expect(page.locator('.tt-row[data-record-id="fixture-compaction"]')).toHaveAttribute('data-timeline-focus', 'outside');
    await page.locator('.tt-row-compacted').click();
    await expect(page.locator('.tt-details .tt-section-scroll')).toContainText('Fixture retained context after compaction.');
    const selected = await geometry();
    expect(selected.model[0].left / selected.width).toBeCloseTo(initial.model[0].left / initial.width, 2);
    expect(selected.model[1].left / selected.width).toBeCloseTo(initial.model[1].left / initial.width, 2);
    releaseOlder();
    await page.locator('.tt-ledger').evaluate(ledger => { ledger.scrollTop = 0; ledger.dispatchEvent(new Event('scroll')); });
    await expect(page.locator('.tt-row')).toHaveCount(4);
    const paged = await geometry();
    // Opening the inspector changes available width. Compare fractions.
    for (let index = 0; index < initial.model.length; index += 1) {
      expect(paged.model[index].left / paged.width).toBeCloseTo(initial.model[index].left / initial.width, 2);
      expect(paged.model[index].width / paged.width).toBeCloseTo(initial.model[index].width / initial.width, 2);
    }
    expect(errors).toEqual([]);
  } finally { await service.dispose(); }
});
