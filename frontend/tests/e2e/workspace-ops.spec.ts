import { test, expect, type APIRequestContext, type Locator, type Page } from '@playwright/test';

/**
 * 云端工作目录（左栏文件区）· 方案 §6.2「文件操作清单」真机逐项走查。
 *
 * 为什么要有这条用例：`docs/云端易用重构方案.md` §6.2 的文件操作清单此前只有"实现存在"，
 * 没有一条端到端用例在**真实浏览器 + 真实服务端 + 真实文件系统**上逐项点过。
 * 本用例串行覆盖清单每一项，每项都断言**用户可见的结果**（文件树行、用量读数数字、
 * download 事件、ElMessageBox、预览弹层正文、输入区引用 chip），不靠截图、
 * 不靠"没有报错"，也不用 `waitForTimeout` 兜时间（一律 poll / toBeVisible 轮询）。
 *
 * 关键实现事实（读源码 + 真机核对，供后续维护）：
 * - 文件区根：`.messenger-sidebar-files-region`；面板：`.workspace-files`；
 *   上传 input：`.messenger-sidebar-files-region input[type="file"]`（隐藏，可直接 setInputFiles）。
 * - 工具栏按钮用中文 `aria-label`：上传到当前目录 / 刷新 / 多选（多选态变「退出多选」）。
 * - 行菜单是**自绘**的：`contextmenu`（或行内 `⋯` 按钮 `.workspace-file-menu`）打开
 *   `.workspace-files-menu`（Teleport 到 body），菜单项 `.workspace-files-menu-item`。
 *   注意：`.workspace-file-menu` 是行内那个省略号按钮本身，**不是**菜单弹层。
 * - 行菜单命令：文件行 = 预览/下载/重命名/引用到聊天/删除；
 *   目录行 = 打开/打包下载/新建目录/新建文件/重命名/引用到聊天/删除（**文件行没有「打包下载」**）。
 * - 右击树区**空白处**呼出同一个菜单（没有宿主行）：新建目录/新建文件落在当前目录，
 *   并承接原头部 `⋯`（已下线）的三项：打包下载当前目录 / 刷新用量统计 / 清空工作目录。
 * - 移动/复制已从行菜单下线（重命名仍走 `/workspace/move`）；用量读数从底部搬到标题行右侧。
 * - 预览弹层：`.workspace-dialog.workspace-dialog--file-preview`（append-to-body，挂在 body 下），
 *   正文渲染在 CodeMirror 的 `.cm-content`；关闭按钮 `.messenger-dialog-close`；
 *   仅当 `editable`（文本类）时头部才有「保存」按钮（`.workspace-btn--primary`）。
 * - 引用到聊天 → `ChatComposer` 把它变成 `.workspace-quote-item` chip（名称 + `@相对路径`）。
 * - 用量读数 `.workspace-files-usage`：「已用 {size} · {N} 个文件」，N 是**递归**统计（服务端 walkdir）。
 * - 走查用 `registerAndSeed` 先用 API 铺好「根文件 + 一个含文件的子目录」：新用户根目录里有没有
 *   服务端预置内容取决于智能体内视层的落地时机，不能当前提（基线文件数仍从用量读数实时取）。
 *
 * 环境：真实请求一律走 baseURL（vite dev server 代理 /wunder），与 shell-layout.spec.ts 一致，
 * 保证页面与接口指向同一个服务端；`ADMIN_E2E_ORIGIN` 只用于排查时对照。
 */

// 断言全部基于中文文案，必须固定 locale（Playwright 默认 en-US 会拿到英文文案）。
test.use({ locale: 'zh-CN', viewport: { width: 1440, height: 1000 } });

const PASSWORD = 'Passw0rd!23';

// ------------------------------------------------------------------ 小工具

const escapeRegExp = (value: string): string => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
/** 精确整串匹配（避免「下载」误命中「打包下载」这类前缀冲突）。 */
const exactText = (value: string): RegExp => new RegExp(`^${escapeRegExp(value)}$`);

const filesRegion = (page: Page): Locator => page.locator('.messenger-sidebar-files-region').first();
const fileRows = (page: Page): Locator => filesRegion(page).locator('.workspace-file-row');
/** 按文件树里的**显示名**精确定位一行（`.workspace-file-name` 是行的名称 span）。 */
const rowByName = (page: Page, name: string): Locator =>
  fileRows(page).filter({
    has: page.locator('.workspace-file-name').filter({ hasText: exactText(name) })
  });
const rowNames = (page: Page): Promise<string[]> =>
  filesRegion(page).locator('.workspace-file-name').allInnerTexts();
/** 行的**深度缩进**（px）。WorkspaceFileTree 用 `padding-left: 6 + depth*12` 表达层级，根层 = 6，子目录内 = 18。 */
const rowIndents = async (page: Page, name: string): Promise<number[]> =>
  (await rowByName(page, name).evaluateAll((nodes) =>
    nodes.map((node) => Number.parseInt((node as HTMLElement).style.paddingLeft || '0', 10))
  )).sort((left, right) => left - right);
const usageBar = (page: Page): Locator => filesRegion(page).locator('.workspace-files-usage');
/** 工具栏按钮：容器是 `.messenger-sidebar-files-toolbar`，按钮用中文 aria-label 区分。 */
const toolbar = (page: Page): Locator => filesRegion(page).locator('.messenger-sidebar-files-toolbar');
const toolbarButton = (page: Page, label: string): Locator =>
  toolbar(page).locator(`button[aria-label="${label}"]`);

const rowMenu = (page: Page): Locator => page.locator('.workspace-files-menu');
const rowMenuItem = (page: Page, label: string): Locator =>
  rowMenu(page).locator('.workspace-files-menu-item').filter({ hasText: exactText(label) });

const messageBox = (page: Page): Locator => page.locator('.el-message-box');
const messageBoxButton = (page: Page, label: string): Locator =>
  page.locator('.el-message-box__btns button').filter({ hasText: exactText(label) });

const previewDialog = (page: Page): Locator =>
  page.locator('.workspace-dialog.workspace-dialog--file-preview');
/** ElMessage 提示：同时可能有上一条还在淡出，取最新一条。 */
const toast = (page: Page, text: string): Locator =>
  page.locator('.el-message').filter({ hasText: new RegExp(escapeRegExp(text)) }).last();

/** 用量读数上的「N 个文件」→ N（递归口径，含服务端预置的 worker-card 目录）。 */
const readFileCount = async (page: Page): Promise<number> => {
  const text = (await usageBar(page).innerText()).replace(/\s+/g, ' ');
  const matched = /(\d+)\s*个文件/.exec(text);
  return matched ? Number(matched[1]) : Number.NaN;
};

/** 上传：直接给隐藏 input 喂内存文件，不需要真实磁盘文件。 */
const uploadFile = async (page: Page, name: string, body: string): Promise<void> => {
  await filesRegion(page).locator('input[type="file"]').setInputFiles({
    name,
    mimeType: 'text/markdown',
    buffer: Buffer.from(body, 'utf-8')
  });
};

/** 右键打开**自绘**行菜单并点击指定项；返回时菜单已收起。 */
const runRowCommand = async (page: Page, name: string, label: string): Promise<void> => {
  const row = rowByName(page, name);
  await expect(row, `文件树应存在行「${name}」（当前行：${(await rowNames(page)).join(' | ')}）`).toBeVisible({
    timeout: 20_000
  });
  await row.click({ button: 'right' });
  const menu = rowMenu(page);
  await expect(menu, `右键「${name}」应弹出 .workspace-files-menu`).toBeVisible({ timeout: 10_000 });
  const item = rowMenuItem(page, label);
  await expect(item, `行菜单应含「${label}」，实际项：${(await menu.innerText()).replace(/\s+/g, ' ')}`).toBeVisible();
  await item.click();
  await expect(menu, '选择菜单项后菜单应收起').toBeHidden({ timeout: 10_000 });
};

/** 行菜单里会触发浏览器下载的项（下载 / 打包下载），返回 download 事件。 */
const runRowCommandForDownload = async (
  page: Page,
  name: string,
  label: string
): Promise<{ suggestedFilename: () => string }> => {
  const [download] = await Promise.all([
    page.waitForEvent('download', { timeout: 30_000 }),
    runRowCommand(page, name, label)
  ]);
  return download;
};

/**
 * 右击文件树**空白处**（最后一行之下）呼出同一个自绘菜单并点击指定项。
 * 落点刻意取 `.workspace-files-rows` 之下，命中行会弹出行菜单而不是空白菜单。
 */
const openBlankMenu = async (page: Page): Promise<Locator> => {
  const scroller = filesRegion(page).locator('.workspace-files-scroll');
  await expect(scroller, '文件树滚动容器应存在').toBeVisible({ timeout: 20_000 });
  const scrollerBox = await scroller.boundingBox();
  expect(scrollerBox, '应能量出文件树滚动容器的位置').not.toBeNull();
  const rowsBox = await filesRegion(page).locator('.workspace-files-rows').boundingBox();
  const blankTop = rowsBox ? rowsBox.y + rowsBox.height - scrollerBox!.y : 8;
  expect(
    blankTop < scrollerBox!.height - 4,
    `文件树内容已铺满滚动区（空白起点 ${blankTop}，高度 ${scrollerBox!.height}），没有空白可右击`
  ).toBeTruthy();
  await scroller.click({ button: 'right', position: { x: 20, y: Math.max(8, blankTop + 10) } });
  const menu = rowMenu(page);
  await expect(menu, '右击空白处应弹出 .workspace-files-menu').toBeVisible({ timeout: 10_000 });
  return menu;
};

const runBlankCommand = async (page: Page, label: string): Promise<void> => {
  const menu = await openBlankMenu(page);
  const item = rowMenuItem(page, label);
  await expect(
    item,
    `空白菜单应含「${label}」，实际项：${(await menu.innerText()).replace(/\s+/g, ' ')}`
  ).toBeVisible();
  await item.click();
  await expect(menu, '选择菜单项后菜单应收起').toBeHidden({ timeout: 10_000 });
};

const runBlankCommandForDownload = async (
  page: Page,
  label: string
): Promise<{ suggestedFilename: () => string }> => {
  const [download] = await Promise.all([
    page.waitForEvent('download', { timeout: 30_000 }),
    runBlankCommand(page, label)
  ]);
  return download;
};

/** 断言行菜单**不含**指定项，然后 Esc 收起（用于钉住已下线的移动/复制）。 */
const expectRowMenuLacks = async (page: Page, name: string, labels: string[]): Promise<void> => {
  const row = rowByName(page, name);
  await expect(row, `文件树应存在行「${name}」`).toBeVisible({ timeout: 20_000 });
  await row.click({ button: 'right' });
  const menu = rowMenu(page);
  await expect(menu).toBeVisible({ timeout: 10_000 });
  const items = (await menu.innerText()).replace(/\s+/g, ' ');
  for (const label of labels) {
    await expect(rowMenuItem(page, label), `行菜单不应再有「${label}」，实际项：${items}`).toHaveCount(0);
  }
  await page.keyboard.press('Escape');
  await expect(menu, 'Esc 应收起行菜单').toBeHidden({ timeout: 10_000 });
};

/** 从页面存储里读当前 access_token（做服务端复核用，不注入任何 token）。 */
const readPageAccessToken = async (page: Page): Promise<string> =>
  page.evaluate(() =>
    String(
      window.sessionStorage.getItem('access_token') || window.localStorage.getItem('access_token') || ''
    ).trim()
  );

/**
 * 直接问服务端某个目录下有哪些条目。UI 断言之外的**独立证据**：
 * 复制/移动/删除/清空都是"服务端文件系统真的变了"，不能只看前端有没有把行抹掉。
 */
const serverEntryNames = async (
  page: Page,
  request: APIRequestContext,
  path = ''
): Promise<string[]> => {
  const token = await readPageAccessToken(page);
  expect(token, '应能从页面存储读到 access_token 以做服务端复核').not.toBe('');
  const response = await request.get(
    `/wunder/workspace?path=${encodeURIComponent(path)}&offset=0&limit=200&sort_by=name&order=asc`,
    { headers: { Authorization: `Bearer ${token}` } }
  );
  expect(response.ok(), `服务端列出 ${path || '<root>'} 失败：${response.status()}`).toBeTruthy();
  const payload = (await response.json()) as { entries?: Array<{ name?: string }> };
  return (payload.entries || []).map((entry) => String(entry.name || ''));
};

/** ElMessageBox.prompt：填入值后点确认，并等弹窗真正关闭（校验不过时它会保持打开）。 */
const answerPrompt = async (page: Page, confirmLabel: string, value: string): Promise<void> => {
  const box = messageBox(page);
  await expect(box, '应弹出 ElMessageBox').toBeVisible({ timeout: 10_000 });
  const input = box.locator('.el-message-box__input input');
  await expect(input, 'prompt 型 ElMessageBox 应有输入框（.el-message-box__input input）').toBeVisible();
  await input.fill(value);
  const confirm = messageBoxButton(page, confirmLabel);
  await expect(confirm, `ElMessageBox 应有「${confirmLabel}」按钮`).toBeVisible();
  await confirm.click();
  await expect(box, `点「${confirmLabel}」后弹窗应关闭（未关闭通常说明 inputValidator 拦下了）`).toBeHidden({
    timeout: 10_000
  });
};

/** ElMessageBox.confirm：只点确认。 */
const answerConfirm = async (page: Page, confirmLabel: string): Promise<void> => {
  const box = messageBox(page);
  await expect(box, '应弹出二次确认 ElMessageBox').toBeVisible({ timeout: 10_000 });
  const confirm = messageBoxButton(page, confirmLabel);
  await expect(confirm, `二次确认应有「${confirmLabel}」按钮`).toBeVisible();
  await confirm.click();
  await expect(box).toBeHidden({ timeout: 10_000 });
};

const isDirExpanded = async (page: Page, name: string): Promise<boolean> =>
  (await rowByName(page, name).getAttribute('aria-expanded')) === 'true';

/** 展开目录行（已展开则不动）。 */
const expandDir = async (page: Page, name: string): Promise<void> => {
  const row = rowByName(page, name);
  await expect(row, `目录行「${name}」应可见`).toBeVisible({ timeout: 20_000 });
  if (!(await isDirExpanded(page, name))) {
    await row.click();
  }
  await expect(row, `目录行「${name}」应处于展开态`).toHaveAttribute('aria-expanded', 'true', {
    timeout: 20_000
  });
};

const closePreview = async (page: Page): Promise<void> => {
  await previewDialog(page).locator('.messenger-dialog-close').click();
  await expect(previewDialog(page), '关闭后预览弹层应消失').toBeHidden({ timeout: 10_000 });
};

/**
 * 注册一个新用户，并用 API 铺好最小目录结构：根一个文件 + 一个含文件的子目录。
 * 走查不能靠服务端预置内容起跑：新账号的工作目录现在可能是空的
 * （agents/global/knowledge/skills 由智能体内视层在更晚的时机才落地）。
 */
type WorkspaceSeed = { rootFile: string; dirName: string; innerFile: string };

const registerAndSeed = async (
  request: APIRequestContext,
  username: string
): Promise<WorkspaceSeed> => {
  const suffix = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
  const seed: WorkspaceSeed = {
    rootFile: `seed-${suffix}.md`,
    dirName: `seed-dir-${suffix}`,
    innerFile: `seed-inner-${suffix}.md`
  };

  const registered = await request.post('/wunder/auth/register', {
    data: { username, password: PASSWORD }
  });
  expect(registered.ok(), `register failed: ${registered.status()}`).toBeTruthy();
  const payload = (await registered.json()) as { data?: { access_token?: string } };
  const token = String(payload.data?.access_token || '');
  expect(token, '注册应返回 access_token 以便用 API 铺种子文件').not.toBe('');
  const headers = { Authorization: `Bearer ${token}` };

  const makeDir = await request.post('/wunder/workspace/dir', { headers, data: { path: seed.dirName } });
  expect(makeDir.ok(), `种子目录创建失败：${makeDir.status()}`).toBeTruthy();

  const uploads: Array<[string, string]> = [
    ['', seed.rootFile],
    [seed.dirName, seed.innerFile]
  ];
  for (const [path, name] of uploads) {
    const uploaded = await request.post('/wunder/workspace/upload', {
      headers,
      multipart: {
        path,
        files: { name, mimeType: 'text/markdown', buffer: Buffer.from(`# ${name}\n`, 'utf-8') }
      }
    });
    expect(uploaded.ok(), `种子文件 ${name} 上传失败：${uploaded.status()}`).toBeTruthy();
  }
  return seed;
};

// -------------------------------------------------------------------- 用例

test('云端工作目录：§6.2 文件操作清单逐项真机走查', async ({ page, request }) => {
  test.setTimeout(600_000);

  const nonce = `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
  const username = `e2e_ops_${nonce}`;
  const rootFileA = `note-a-${nonce}.md`;
  const rootFileB = `note-b-${nonce}.md`;
  const dirName = `docs-${nonce}`;
  const innerFile = `inner-${nonce}.md`;
  const renamedFile = `renamed-a-${nonce}.md`;
  const editFile = `edit-${nonce}.md`;
  const delFile = `del-${nonce}.md`;
  const markerA = `BODYROOTA${nonce}`;
  const markerInner = `BODYINNER${nonce}`;
  const markerEdited = `EDITED${nonce}`;
  const body = (marker: string) => `# ${marker}\n\n${marker} 正文内容\n\n- 第一行\n- 第二行\n`;

  // 页面异常收集：只作为附加卫生断言，不替代任何功能断言。
  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));

  // 记录目录列表请求，用于断言「目录懒加载」真的按需拉取（GET /wunder/workspace?path=...）。
  type ListCall = { path: string };
  const listCalls: ListCall[] = [];
  page.on('request', (outgoing) => {
    if (outgoing.method() !== 'GET') return;
    let parsed: URL;
    try {
      parsed = new URL(outgoing.url());
    } catch {
      return;
    }
    if (parsed.pathname !== '/wunder/workspace') return;
    listCalls.push({ path: parsed.searchParams.get('path') || '' });
  });
  const listCallCount = (path: string): number => listCalls.filter((call) => call.path === path).length;

  // ---------------------------------------------------------------- 前置
  const seed = await registerAndSeed(request, username);

  await page.addInitScript(() => localStorage.setItem('wunder_language', 'zh-CN'));
  await page.goto('/login');
  // 登录表单用 class 定位（与文案无关）：中文/英文 locale 都能跑。
  await page.locator('.auth-input[type="text"]').first().fill(username);
  await page.locator('.auth-input[type="password"]').first().fill(PASSWORD);
  await page.locator('.auth-submit-btn').first().click();

  await expect(page.locator('.messenger-view').first(), '登录后应进入两栏工作台').toBeVisible({
    timeout: 30_000
  });
  await expect(filesRegion(page), '左栏文件区应存在').toBeVisible({ timeout: 30_000 });
  await expect(filesRegion(page).locator('.workspace-files')).toBeVisible();

  // 中文文案前置：下面所有断言都基于中文 aria-label / 菜单项，
  // 若这里失败说明 locale 没生效，报错立刻能定位到原因。
  await expect(
    toolbarButton(page, '上传到当前目录'),
    '文件区工具栏应为中文文案（上传到当前目录）'
  ).toBeVisible({ timeout: 20_000 });

  // 初次挂载确实按需请求了根目录列表。
  await expect
    .poll(() => listCallCount(''), {
      timeout: 20_000,
      message: '初次挂载应请求根目录列表 GET /wunder/workspace?path='
    })
    .toBeGreaterThan(0);

  // ⚠ 已知产品缺陷（详见文件末尾 fixme 用例）：首屏 loadDirectory('') 走的是
  // `useWorkspaceFileTree.ensureDirectory()` 首次创建分支，它返回的是**未经 reactive 包装的
  // 原始 state 对象**（对象刚 set 进 reactive Map，直接 return 局部变量），因此随后
  // `state.entries = ...` / `state.loading = false` 都不会触发视图更新：
  // 文件树会永久停在「加载中...」，直到同一目录被第二次加载（`directories.get()` 这次
  // 返回的是 reactive 代理）为止 —— 也就是用户点一次工具栏「刷新」。
  // 这里点一次「刷新」以继续走查（缺陷修好后这一下是无害的多余刷新）；
  // 缺陷本身由末尾的 fixme 用例钉住，并按实测结果打 annotation。
  const rowsAtFirstPaint = await fileRows(page).count();
  test
    .info()
    .annotations.push(
      rowsAtFirstPaint === 0
        ? {
            type: '已知缺陷-首屏文件树',
            description: '首屏 .workspace-files-state 停在「加载中...」且 0 行；靠工具栏「刷新」自救'
          }
        : { type: '首屏文件树', description: `首屏正常渲染 ${rowsAtFirstPaint} 行` }
    );
  await toolbarButton(page, '刷新').click();
  await expect
    .poll(async () => (await rowNames(page)).length, {
      timeout: 20_000,
      message: '「刷新」后文件树应渲染出根目录条目'
    })
    .toBeGreaterThan(0);

  // ============================================================ §6.2-1 列表/浏览 + 目录懒加载
  // (a) 列表：根目录应列出 API 铺下的种子文件；服务端预置目录何时落地只做记录，不作硬断言。
  const rootNames = await rowNames(page);
  expect(rootNames, `根目录列表应包含种子文件 ${seed.rootFile}`).toContain(seed.rootFile);
  const presetDirs = ['agents', 'global', 'knowledge', 'skills'].filter((name) => rootNames.includes(name));
  test.info().annotations.push({
    type: '根目录预置内容',
    description: presetDirs.length
      ? `服务端预置目录已出现在根：${presetDirs.join('/')}`
      : '服务端预置目录未出现在根（内视层更晚落地），走查不依赖它'
  });
  await expect(usageBar(page), '用量读数应渲染在标题行右侧').toBeVisible({ timeout: 20_000 });
  await expect(
    usageBar(page).locator('xpath=..'),
    '用量读数应已从文件区底部搬进标题行（父级是 .messenger-sidebar-files-head）'
  ).toHaveClass(/messenger-sidebar-files-head/);
  const baselineFiles = await readFileCount(page);
  expect(Number.isFinite(baselineFiles), `用量读数应能读出文件数，实际：${await usageBar(page).innerText()}`).toBe(
    true
  );
  expect(baselineFiles, '基线文件数应大于 0（预置 worker-card）').toBeGreaterThan(0);

  // 上传/新建/删除都会动递归文件数，用运行计数器断言，避免和「新建文件」的步数耦合。
  let expectedFiles = baselineFiles;
  const expectFileCount = async (message: string): Promise<void> => {
    await expect
      .poll(() => readFileCount(page), { timeout: 30_000, message: `${message}（应为 ${expectedFiles}）` })
      .toBe(expectedFiles);
  };

  // ============================================================ §6.2-2 上传
  await uploadFile(page, rootFileA, body(markerA));
  await expect(rowByName(page, rootFileA), `上传后 ${rootFileA} 应出现在文件树`).toBeVisible({
    timeout: 30_000
  });
  // 上传完成后用量数字必须变化（这里是递归文件数 +1）。
  expectedFiles += 1;
  await expectFileCount(`上传 ${rootFileA} 后`);

  await uploadFile(page, rootFileB, body(`BODYROOTB${nonce}`));
  await expect(rowByName(page, rootFileB), `上传后 ${rootFileB} 应出现在文件树`).toBeVisible({
    timeout: 30_000
  });
  expectedFiles += 1;
  await expectFileCount('第二次上传后');

  // ============================================================ §6.2-5 新建目录（空白右键菜单）
  await runBlankCommand(page, '新建目录');
  await answerPrompt(page, '确认', dirName);
  await expect(rowByName(page, dirName), `新建目录后 ${dirName} 应出现在文件树`).toBeVisible({
    timeout: 20_000
  });
  await expect(toast(page, '目录已创建'), '新建目录应有成功提示').toBeVisible({ timeout: 10_000 });

  // 目录刚建好、**尚未展开**时不得请求它的内容（懒加载）。
  expect(listCallCount(dirName), '目录未展开前不应请求其内容（懒加载）').toBe(0);

  const dirRow = rowByName(page, dirName);
  expect(await dirRow.getAttribute('aria-expanded'), '新建目录默认应是收起态').not.toBe('true');
  await dirRow.click();
  await expect(dirRow, '点击目录行后应展开').toHaveAttribute('aria-expanded', 'true', { timeout: 20_000 });
  await expect
    .poll(() => listCallCount(dirName), {
      timeout: 20_000,
      message: `展开 ${dirName} 时应按需请求该目录（GET /wunder/workspace?path=${dirName}）`
    })
    .toBeGreaterThan(0);

  // 展开后往里上传：子文件行应出现在目录行**之后**（深度缩进更大）。
  await uploadFile(page, innerFile, body(markerInner));
  await expect(rowByName(page, innerFile), `上传后子目录内应看到 ${innerFile}`).toBeVisible({
    timeout: 30_000
  });
  expectedFiles += 1;
  await expectFileCount('子目录上传后');

  const domOrder = await rowNames(page);
  expect(domOrder.indexOf(innerFile), `${innerFile} 应排在 ${dirName} 之后（子行缩进）`).toBeGreaterThan(
    domOrder.indexOf(dirName)
  );
  const indentOf = async (name: string) =>
    Number.parseInt(
      (await rowByName(page, name).evaluate((el) => (el as HTMLElement).style.paddingLeft)) || '0',
      10
    );
  expect(await indentOf(innerFile), '子文件行缩进应大于父目录行（depth 生效）').toBeGreaterThan(
    await indentOf(dirName)
  );

  // 收起 → 子行消失；再展开 → 子行回来，且**不再重复请求**（已缓存的目录不重拉）。
  const callsBeforeCollapse = listCallCount(dirName);
  await rowByName(page, dirName).click();
  await expect(rowByName(page, dirName)).toHaveAttribute('aria-expanded', 'false');
  await expect(rowByName(page, innerFile), '收起目录后子文件行应消失').toHaveCount(0);
  await rowByName(page, dirName).click();
  await expect(rowByName(page, dirName)).toHaveAttribute('aria-expanded', 'true');
  await expect(rowByName(page, innerFile), '再次展开后子文件行应重新出现').toBeVisible({ timeout: 20_000 });
  expect(listCallCount(dirName), '已加载过的目录再次展开不应重复请求（懒加载只拉一次）').toBe(
    callsBeforeCollapse
  );

  // ============================================================ §6.2-5b 新建文件（目录行右键菜单）
  const createdFile = `created-${nonce}.md`;
  await runRowCommand(page, dirName, '新建文件');
  await answerPrompt(page, '确认', createdFile);
  await expect(rowByName(page, createdFile), `新建文件后 ${createdFile} 应出现在 ${dirName} 内`).toBeVisible({
    timeout: 20_000
  });
  await expect(toast(page, '文件已创建'), '新建文件应有成功提示').toBeVisible({ timeout: 10_000 });
  expect(
    await rowIndents(page, createdFile),
    `新建的行应落在 ${dirName} 内（缩进 18），而不是根层`
  ).toEqual([18]);
  expectedFiles += 1;
  await expectFileCount(`新建 ${createdFile} 后`);
  expect(await serverEntryNames(page, request, dirName), `服务端 ${dirName} 应有新建的文件`).toContain(
    createdFile
  );

  // ============================================================ §6.2-7/8 移动到…／复制到…（已下线）
  // 行菜单不再提供跨目录移动/复制（重命名仍走 /workspace/move）；这里钉住菜单项边界。
  await expectRowMenuLacks(page, rootFileB, ['移动到…', '复制到…']);
  await expectRowMenuLacks(page, dirName, ['移动到…', '复制到…']);

  // ============================================================ §6.2-3 下载单文件
  const single = await runRowCommandForDownload(page, rootFileA, '下载');
  expect(single.suggestedFilename(), '单文件下载的 suggestedFilename 应含该文件名').toContain(
    rootFileA.replace(/\.md$/, '')
  );
  expect(single.suggestedFilename()).toMatch(/\.md$/);

  // ============================================================ §6.2-4 打包下载（目录行 + 空白菜单当前目录）
  const dirArchive = await runRowCommandForDownload(page, dirName, '打包下载');
  expect(dirArchive.suggestedFilename(), '目录行打包下载的文件名应以 .zip 结尾').toMatch(/\.zip$/i);
  expect(dirArchive.suggestedFilename()).toContain(dirName);

  const currentArchive = await runBlankCommandForDownload(page, '打包下载当前目录');
  expect(
    currentArchive.suggestedFilename(),
    '空白菜单「打包下载当前目录」应触发 zip 下载'
  ).toMatch(/\.zip$/i);

  // ============================================================ §6.2-6 重命名
  await runRowCommand(page, rootFileA, '重命名');
  await answerPrompt(page, '确认', renamedFile);
  await expect(rowByName(page, renamedFile), `重命名后应出现新名 ${renamedFile}`).toBeVisible({
    timeout: 20_000
  });
  await expect(rowByName(page, rootFileA), `重命名后旧名 ${rootFileA} 应消失`).toHaveCount(0);
  await expect(toast(page, '已重命名')).toBeVisible({ timeout: 10_000 });
  expect(await serverEntryNames(page, request), `服务端根目录应有重命名后的 ${renamedFile}`).toContain(
    renamedFile
  );

  // ============================================================ §6.2-9 批量（多选后批量删除）
  const selectToggle = toolbar(page).locator('button.is-toggle');
  await expect(selectToggle, '工具栏应有「多选」按钮').toHaveAttribute('aria-label', '多选');
  await selectToggle.click();
  await expect(selectToggle, '进入多选后按钮应变为「退出多选」').toHaveAttribute('aria-label', '退出多选');
  await expect(selectToggle).toHaveAttribute('aria-pressed', 'true');

  // 只勾目录内的 innerFile，验证未被勾选的行不被误删。
  await expandDir(page, dirName);
  const checkOf = (name: string) => rowByName(page, name).locator('input.workspace-file-check');
  await expect(checkOf(innerFile), '多选态下每行应有复选框').toHaveCount(1);
  await checkOf(innerFile).click();
  await expect(checkOf(innerFile)).toBeChecked();
  await expect(filesRegion(page).locator('.workspace-files-selection-count')).toContainText('已选 1 项');

  await filesRegion(page).locator('.workspace-files-selection button.is-danger').click();
  await answerConfirm(page, '删除');
  await expect(rowByName(page, innerFile), `批量删除后 ${innerFile} 应消失`).toHaveCount(0, {
    timeout: 20_000
  });
  await expect(rowByName(page, createdFile), '未被勾选的新建文件不应被批量删除带走').toBeVisible({
    timeout: 20_000
  });
  await expect(rowByName(page, dirName), '目录本身不应被删除').toBeVisible();
  expectedFiles -= 1;
  await expectFileCount(`批量删除 ${innerFile} 后`);
  expect(await serverEntryNames(page, request, dirName), `服务端 ${dirName} 应只剩 ${createdFile}`).toEqual([
    createdFile
  ]);
  await expect(selectToggle, '批量删除后应自动退出多选').toHaveAttribute('aria-label', '多选');

  // ============================================================ §6.2-10 删除单文件
  // 先补两个文件：delFile 用于单文件删除，editFile 留给后面的预览/编辑/引用。
  // 左击一个根层文件行把上传目标钉回根目录（点目录行会把目标切到该目录）。
  await rowByName(page, rootFileB).click();
  await uploadFile(page, editFile, body(`BODYEDIT${nonce}`));
  await expect(rowByName(page, editFile)).toBeVisible({ timeout: 30_000 });
  await uploadFile(page, delFile, body(`BODYDEL${nonce}`));
  await expect(rowByName(page, delFile)).toBeVisible({ timeout: 30_000 });
  expectedFiles += 2;
  await expectFileCount('补传 editFile / delFile 后');

  await runRowCommand(page, delFile, '删除');
  await expect(messageBox(page), '删除单文件应弹二次确认，且确认文案含文件名').toContainText(delFile);
  await answerConfirm(page, '删除');
  await expect(rowByName(page, delFile), `删除后 ${delFile} 应消失`).toHaveCount(0, { timeout: 20_000 });
  await expect(rowByName(page, editFile), '删除只应影响目标文件').toBeVisible();
  await expect(toast(page, '已删除')).toBeVisible({ timeout: 10_000 });
  expectedFiles -= 1;
  await expectFileCount(`删除 ${delFile} 后`);
  expect(await serverEntryNames(page, request), `服务端根目录不应再有 ${delFile}`).not.toContain(delFile);

  // ============================================================ §6.2-12 预览
  await runRowCommand(page, editFile, '预览');
  const dialog = previewDialog(page);
  await expect(dialog, '预览应出现 .workspace-dialog--file-preview 弹层').toBeVisible({ timeout: 20_000 });
  await expect(dialog.locator('.messenger-dialog-header')).toContainText('文件预览');
  await expect(dialog.locator('.messenger-dialog-header')).toContainText(editFile);
  await expect(dialog.locator('.cm-content'), '预览正文应包含上传时的内容').toContainText(
    `BODYEDIT${nonce}`,
    { timeout: 20_000 }
  );
  await expect(dialog.locator('.workspace-preview-hint')).toContainText(editFile);

  // ============================================================ §6.2-13 在线编辑 + 引用到聊天
  const saveButton = dialog.locator('.workspace-btn--primary');
  await expect(saveButton, '文本类（可编辑）文件预览才有的「保存」按钮应可见').toBeVisible();
  await expect(saveButton).toContainText('保存');
  await expect(saveButton, '未修改时保存按钮应禁用').toBeDisabled();

  await dialog.locator('.cm-content').click();
  await page.keyboard.press('Control+End');
  await page.keyboard.type(`\n${markerEdited}\n`);
  await expect(saveButton, '编辑后保存按钮应可用').toBeEnabled({ timeout: 10_000 });
  await saveButton.click();
  await expect(toast(page, '已保存'), '保存应给出成功提示').toBeVisible({ timeout: 20_000 });

  // 真实往返：关掉再打开，读到的应是服务端保存后的内容。
  await closePreview(page);
  await runRowCommand(page, editFile, '预览');
  await expect(previewDialog(page).locator('.cm-content'), '重新打开后应读到保存后的内容').toContainText(
    markerEdited,
    { timeout: 20_000 }
  );
  await closePreview(page);

  // 引用到聊天：行菜单 → 输入区出现引用 chip。
  await runRowCommand(page, editFile, '引用到聊天');
  await expect(toast(page, '已引用到聊天输入区'), '引用成功应有提示').toBeVisible({ timeout: 10_000 });
  const quoteChip = page.locator('.workspace-quote-item');
  await expect(quoteChip, '聊天输入区应出现引用 chip').toHaveCount(1, { timeout: 20_000 });
  await expect(quoteChip).toContainText(editFile);
  await expect(quoteChip.locator('.upload-preview-meta')).toContainText(`@${editFile}`);

  // ============================================================ §6.2-11 清空工作目录（放最后，它会清空整根）
  await runBlankCommand(page, '清空工作目录');
  await expect(messageBox(page), '清空应弹出高危险确认框（含确认语要求）').toContainText('清空');
  await answerPrompt(page, '确认', '清空');

  await expect(toast(page, '工作目录已清空')).toBeVisible({ timeout: 20_000 });
  // 用量读数会立刻回到 0（那是另一条数据通路，正常）。
  await expect(usageBar(page)).toContainText('已用 0 B', { timeout: 20_000 });
  // 但文件树本身又卡在「加载中...」：清空走的是 `tree.reset()` + `loadDirectory('')`，
  // 与首屏完全相同的路径 —— 首次创建 state 又是 raw 对象，所以同样不会自动刷新。
  // （实测：清空后 20s 内 `.workspace-files-state` 一直是「加载中...」，行数为 0。）
  // 这里再点一次「刷新」把树救回来，然后断言空态文案。
  await toolbarButton(page, '刷新').click();
  await expect(
    filesRegion(page).locator('.workspace-files-state.is-empty'),
    '清空后文件树应回到空态文案'
  ).toContainText('当前目录没有文件', { timeout: 20_000 });
  await expect(filesRegion(page).locator('.workspace-files-state.is-empty')).toContainText(
    '拖拽文件到此处即可上传'
  );
  expect(await rowNames(page), '清空后不应残留任何行').toEqual([]);
  await expect
    .poll(() => readFileCount(page), {
      timeout: 30_000,
      message: '清空后用量读数应保持 0 个文件'
    })
    .toBe(0);

  // 不只信 UI：直接问服务端列表，确认真的清空了（含预置目录）。
  expect(await serverEntryNames(page, request), '服务端根目录应已无任何条目').toEqual([]);

  await page.screenshot({ path: '../temp_dir/screens/workspace-ops-cleared.png' });
  expect(pageErrors, `页面 pageerror：${pageErrors.join(' | ')}`).toEqual([]);
});

/**
 * 已知产品缺陷（不是用例问题）——文件树"第一次加载"永远渲染不出来，必须手动刷新一次。
 *
 * 现象 1（首屏）：登录后刚进工作台，`GET /wunder/workspace?path=` 已经 200 返回
 * （实测 XHR loadend、body 560B、含 4 个条目），但 `.workspace-files-scroll` 一直只渲染
 * `<div class="workspace-files-state">…加载中...</div>`，一行都不出；点一次工具栏「刷新」后
 * 4 行才出现。
 *
 * 现象 2（任何目录的首次展开）：根目录刷新好之后，点开一个内含子文件的目录
 * （当时是服务端预置的 `agents/`，现改用 API 铺的 seed 目录），`aria-expanded` 变 true、
 * 列表请求也发了，但 6 秒后行数不变，**子项一个都不出**；再点一次「刷新」才出现。
 * 即：目录懒加载的第一次展开等于白点。
 *
 * 根因（frontend/src/views/messenger/workspace/workspaceFileTree.ts `ensureDirectory`）：
 *   const state = directories.get(key);           // 命中 → Vue 返回 reactive 代理
 *   if (!state) { state = createDirectoryState(...); directories.set(key, state); }
 *   return state;                                 // 首次创建时返回的是**原始对象**
 * `directories` 是 `reactive(new Map())`，`set` 存的是 raw，`get` 才返回 reactive 代理。
 * 首次加载走的是"新建"分支，调用方拿到的 `state` 是 raw 对象，后续
 * `state.entries = …` / `state.loading = false` 全部写在 raw 上，不触发依赖更新：
 * 模板停在 loading 分支、`rows` computed 永不重算。第二次加载同一目录
 * （`directories.get()` 这次拿到代理）就恢复正常 —— 这正是"刷新一下就好"。
 * 同样的路径也出现在 `清空工作目录` 之后（`tree.reset()` + `loadDirectory('')`）。
 *
 * 修法（已落地）：`return directories.get(key) as WorkspaceDirectoryState;`（先 set 再统一从 map 读回代理）。
 *
 * 该缺陷已修复，本用例作为**常驻回归门**：任何让"首次加载/首次展开"退回 raw 对象的改动，
 * 都会让下面两条断言立刻变红。
 */
test('回归门：无需手动刷新即可渲染文件树与首次展开的目录', async ({
  page,
  request
}) => {
  const username = `e2e_ops_first_${Date.now().toString(36)}`;
  const seed = await registerAndSeed(request, username);

  await page.addInitScript(() => localStorage.setItem('wunder_language', 'zh-CN'));
  await page.goto('/login');
  await page.locator('.auth-input[type="text"]').first().fill(username);
  await page.locator('.auth-input[type="password"]').first().fill(PASSWORD);
  await page.locator('.auth-submit-btn').first().click();
  await expect(page.locator('.messenger-view').first()).toBeVisible({ timeout: 30_000 });

  // 现象 1：不做任何手动刷新，只等文件树自己渲染出行。
  await expect(fileRows(page).first(), '首屏应无需手动刷新就渲染出根目录条目').toBeVisible({
    timeout: 20_000
  });
  await expect(filesRegion(page).locator('.workspace-files-state')).toHaveCount(0);

  // 现象 2：第一次展开 API 铺的 seed 目录（内含一个子文件）就该看到子项。
  await expect(usageBar(page)).toContainText('个文件');
  const rootRowsBefore = await fileRows(page).count();
  await rowByName(page, seed.dirName).click();
  await expect(rowByName(page, seed.dirName)).toHaveAttribute('aria-expanded', 'true');
  await expect
    .poll(async () => fileRows(page).count(), {
      timeout: 20_000,
      message: `首次展开 ${seed.dirName} 后应出现其子项（原缺陷：必须再手动刷新一次）`
    })
    .toBeGreaterThan(rootRowsBefore);
  await expect(rowByName(page, seed.innerFile), '首次展开后子文件行应直接可见').toBeVisible({
    timeout: 20_000
  });
});

