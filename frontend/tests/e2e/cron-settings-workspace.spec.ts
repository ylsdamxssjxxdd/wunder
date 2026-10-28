import { expect, test } from '@playwright/test';

test.use({ locale: 'zh-CN', viewport: { width: 1100, height: 750 } });

test('scheduled tasks select a workspace instead of an agent', async ({ page }, info) => {
  const writes: Array<Record<string, any>> = [];
  const jobs: Array<Record<string, any>> = [];
  const errors: string[] = [];
  page.on('pageerror', (error) => errors.push(error.message));
  await page.route('**/wunder/cron/**', async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path.endsWith('/workspaces')) {
      await route.fulfill({ json: { data: { workspaces: [
        { workspace_id: 'workspace-one', name: 'Workspace One' },
        { workspace_id: 'workspace-two', name: 'Workspace Two' }
      ] } } });
    } else if (path.endsWith('/add')) {
      const body = request.postDataJSON();
      writes.push(body);
      jobs.unshift({ ...body.job, job_id: `fixture-${writes.length}`, next_run_at: null });
      await route.fulfill({ json: { data: { job: jobs[0] } } });
    } else if (path.endsWith('/list')) {
      expect(new URL(request.url()).searchParams.has('agent_id')).toBe(false);
      await route.fulfill({ json: { data: { jobs } } });
    } else {
      await route.fulfill({ json: { data: { runs: [] } } });
    }
  });
  await page.goto('/tests/fixtures/cron-settings.html');
  await page.locator('.messenger-cron-toolbar button').last().click();
  const dialog = page.getByRole('dialog');
  const fields = dialog.locator('.messenger-cron-field');
  await expect(fields.first()).toContainText('工作区');
  await fields.first().locator('select').selectOption('workspace-two');
  await dialog.locator('textarea').fill('Summarize the fixture files.');
  await dialog.locator('input[type="datetime-local"]').fill('2099-01-02T09:00');
  await page.screenshot({ path: info.outputPath('workspace-once.png'), animations: 'disabled' });
  await dialog.locator('.el-dialog__footer button').last().click();
  await expect(dialog).not.toBeVisible();
  expect(writes[0].job).toMatchObject({ workspace_id: 'workspace-two', schedule: { kind: 'at' } });
  expect(writes[0].job).not.toHaveProperty('agent_id');
  expect(writes[0].job).not.toHaveProperty('session_id');
  await expect(page.locator('.messenger-cron-detail')).toContainText('Workspace Two');

  await page.locator('.messenger-cron-toolbar button').last().click();
  await dialog.locator('textarea').fill('Repeat the fixture check.');
  await dialog.locator('input[type="datetime-local"]').fill('2099-01-02T09:00');
  await dialog.getByRole('button', { name: '重复', exact: true }).click();
  await dialog.locator('input[type="number"]').fill('25');
  await dialog.locator('.messenger-cron-repeat-row select').selectOption('hour');
  await dialog.locator('.el-dialog__footer button').last().click();
  await expect(page.locator('.el-message--warning')).toBeVisible();
  expect(writes).toHaveLength(1);
  await dialog.locator('input[type="number"]').fill('2');
  await dialog.locator('.el-dialog__footer button').last().click();
  await expect(dialog).not.toBeVisible();
  expect(writes[1].job.schedule).toMatchObject({ kind: 'every', every_ms: 7200000 });
  expect(writes[1].job.workspace_id).toBe('workspace-two');
  expect(errors).toEqual([]);
});

test('cloud workspace is explicit and inaccessible settings show an error', async ({ page }) => {
  let denied = false;
  await page.route('**/wunder/cron/**', async (route) => {
    if (denied) {
      await route.fulfill({ status: 403, json: { detail: 'Forbidden' } });
    } else if (new URL(route.request().url()).pathname.endsWith('/workspaces')) {
      await route.fulfill({ json: { data: { workspaces: [{ workspace_id: '__cloud__', name: 'Cloud workspace' }] } } });
    } else {
      await route.fulfill({ json: { data: { jobs: [] } } });
    }
  });
  await page.goto('/tests/fixtures/cron-settings.html');
  await page.locator('.messenger-cron-toolbar button').last().click();
  const dialog = page.getByRole('dialog');
  await expect(dialog.locator('.messenger-cron-field select')).toHaveValue('__cloud__');
  await dialog.getByRole('button', { name: '取消', exact: true }).click();
  denied = true;
  await page.reload();
  await expect(page.locator('.messenger-cron-detail')).toContainText('权限');
  await expect(page.locator('.messenger-cron-toolbar button').last()).toBeDisabled();
});


test('late run history cannot replace the selected task and pages stay accessible', async ({ page }) => {
  let releaseFirst: (() => void) | undefined;
  let markFirstStarted: (() => void) | undefined;
  const firstStarted = new Promise<void>((resolve) => { markFirstStarted = resolve; });
  const firstReleased = new Promise<void>((resolve) => { releaseFirst = resolve; });
  let firstResponseSent = false;
  await page.route('**/wunder/cron/**', async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith('/workspaces')) {
      await route.fulfill({ json: { data: { workspaces: [{ workspace_id: 'workspace-fixture', name: 'Fixture workspace' }] } } });
    } else if (url.pathname.endsWith('/list')) {
      const nextPage = Number(url.searchParams.get('offset')) > 0;
      const ids = nextPage ? ['third-task'] : ['first-task', 'second-task'];
      await route.fulfill({ json: { data: { jobs: ids.map((id) => ({ job_id: id, name: id, enabled: true, workspace_id: 'workspace-fixture' })), has_more: !nextPage } } });
    } else if (url.pathname.endsWith('/runs')) {
      const id = url.searchParams.get('job_id');
      if (id === 'first-task') { markFirstStarted?.(); await firstReleased; }
      await route.fulfill({ json: { data: { runs: [{ run_id: `run-${id}`, status: 'ok', summary: `history-${id}` }] } } });
      if (id === 'first-task') firstResponseSent = true;
    } else {
      await route.fulfill({ json: { data: {} } });
    }
  });
  await page.goto('/tests/fixtures/cron-settings.html');
  await firstStarted;
  await page.getByRole('button', { name: 'second-task - 已启用', exact: true }).click();
  await expect(page.locator('.messenger-cron-runs')).toContainText('history-second-task');
  releaseFirst?.();
  await expect.poll(() => firstResponseSent).toBe(true);
  await expect(page.locator('.messenger-cron-runs')).toContainText('history-second-task');
  await page.getByRole('button', { name: '下一页', exact: true }).click();
  await expect(page.locator('.messenger-cron-list .messenger-list-item')).toHaveCount(1);
  await expect(page.locator('.messenger-cron-list')).toContainText('third-task');
  await expect(page.getByRole('button', { name: '下一页', exact: true })).toBeDisabled();
  await page.getByRole('button', { name: '上一页', exact: true }).click();
  await expect(page.locator('.messenger-cron-list .messenger-list-item')).toHaveCount(2);
});
