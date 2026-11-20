import { expect, test } from '@playwright/test';

test('tool rows state read line counts and edit change sizes without repeating the path', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('wunder_language', 'zh-CN'));
  await page.goto('/__e2e/composer-b4');
  const rows = await page.evaluate(async () => {
    const source = await (await fetch('/src/components/chat/MessageTimelineToolEntry.vue')).text();
    const runtimeUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    if (!runtimeUrl) throw new Error('Missing fixture Vue runtime');
    const [{ createApp, nextTick, h }, { default: Tool }, model, { createPinia }] = await Promise.all([
      import(runtimeUrl),
      import('/src/components/chat/MessageTimelineToolEntry.vue'),
      import('/src/components/chat/toolTimelineModel.ts'),
      import('/tests/support/piniaFixture.ts')
    ]);
    const t = (key: string) => key;
    let serial = 0;
    const run = (toolName: string, callArgs: unknown, resultData: unknown) => ({
      key: `fixture-run-${serial++}`,
      toolName,
      toolDisplayName: toolName,
      toolRuntimeName: toolName,
      toolFunctionName: '',
      callItem: {
        id: `${toolName}-call`,
        eventType: 'tool_call',
        status: 'completed',
        toolName,
        detail: JSON.stringify({ tool: toolName, args: callArgs })
      },
      outputItem: null,
      resultItem: {
        id: `${toolName}-result`,
        eventType: 'tool_result',
        status: 'completed',
        toolName,
        detail: JSON.stringify({ tool: toolName, ok: true, data: resultData })
      }
    });
    const entries = [
      model.buildTimelineToolEntry(run('读取文件',
        { path: 'docs/使用说明书/zh-CN/tools/exec.md' },
        { content: '>>> exec.md\nline', read_lines: 142, total_lines: 142 }), t),
      model.buildTimelineToolEntry(run('读取文件',
        { path: 'crates/wunder-runtime/src/services/tools/file_tool.rs' },
        { content: '>>> file_tool.rs\nline', read_lines: 120, total_lines: 676 }), t),
      model.buildTimelineToolEntry(run('文本编辑',
        { file_path: 'crates/engine/src/lib.rs', old_string: 'const A = 1;', new_string: 'const A = 2;' },
        { path: 'crates/engine/src/lib.rs', files: [{ path: 'crates/engine/src/lib.rs', action: 'update', added_lines: 10, deleted_lines: 2 }] }), t),
      model.buildTimelineToolEntry(run('写入文件',
        { path: 'crates/engine/src/new.rs', content: 'a\nb\n' },
        { path: 'crates/engine/src/new.rs', files: [{ path: 'crates/engine/src/new.rs', action: 'add', added_lines: 12, deleted_lines: 0 }] }), t)
    ];
    const host = document.createElement('div');
    host.id = 'tool-entry-badge-fixture';
    host.style.cssText = 'position:fixed;inset:0;z-index:10000;width:760px;padding:16px;background:#fbfaf8';
    document.body.append(host);
    const app = createApp({
      render: () => h('div', entries.map((entry) => h(Tool, { entry, open: false })))
    });
    app.use(createPinia());
    app.mount(host);
    await nextTick();
    await nextTick();
    const result = [...host.querySelectorAll('.tl-entry')].map((row) => ({
      chips: [...row.querySelectorAll('.tl-entry-chip')].map((node) => node.textContent?.trim() || ''),
      diff: [...row.querySelectorAll('.tl-entry-diff span')].map((node) => node.textContent?.trim() || ''),
      read: row.querySelector('.tl-entry-read-lines')?.textContent?.trim() || '',
      summary: row.querySelector('.tl-entry-summary')?.textContent?.trim() || ''
    }));
    return result;
  });
  expect(rows).toHaveLength(4);
  expect(rows[0].read).toBe('142 行');
  expect(rows[0].summary).toBe('');
  expect(rows[1].read).toBe('120/676 行');
  expect(rows[1].summary).toBe('');
  expect(rows[2].diff).toEqual(['+10', '−2']);
  expect(rows[2].summary).toBe('');
  expect(rows[3].diff).toEqual(['+12']);
  expect(rows[3].summary).toBe('');
});

test('write entries keep the file name chip for absolute paths and cap the diff viewport', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('wunder_language', 'zh-CN'));
  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const source = await (await fetch('/src/components/chat/MessageTimelineToolEntry.vue')).text();
    const runtimeUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    if (!runtimeUrl) throw new Error('Missing fixture Vue runtime');
    const [{ createApp, nextTick, h }, { default: Tool }, model, patch, { createPinia }] = await Promise.all([
      import(runtimeUrl),
      import('/src/components/chat/MessageTimelineToolEntry.vue'),
      import('/src/components/chat/toolTimelineModel.ts'),
      import('/src/components/chat/toolTimelinePatch.ts'),
      import('/tests/support/piniaFixture.ts')
    ]);
    const t = (key: string) => key;
    const absolutePath = '/workspaces/local_user/workspace/temp/sample.py';
    const content = Array.from({ length: 120 }, (_, index) => `line ${index + 1}`).join('\n');
    const run = {
      key: 'fixture-write-absolute',
      toolName: '写入文件',
      toolDisplayName: '写入文件',
      toolRuntimeName: '写入文件',
      toolFunctionName: '',
      callItem: {
        id: 'write-call',
        eventType: 'tool_call',
        status: 'completed',
        toolName: '写入文件',
        detail: JSON.stringify({ tool: '写入文件', args: { path: absolutePath, content } })
      },
      outputItem: null,
      resultItem: {
        id: 'write-result',
        eventType: 'tool_result',
        status: 'completed',
        toolName: '写入文件',
        detail: JSON.stringify({
          tool: '写入文件',
          ok: true,
          data: { path: absolutePath, files: [{ path: absolutePath, action: 'add', added_lines: 120, deleted_lines: 0 }] }
        })
      }
    };
    const entry = model.buildTimelineToolEntry(run, t);
    const patchView = patch.buildTimelinePatchView(run, t);
    const host = document.createElement('div');
    host.id = 'tool-entry-absolute-fixture';
    host.style.cssText = 'position:fixed;inset:0;z-index:10000;width:760px;padding:16px;background:#fbfaf8';
    document.body.append(host);
    const app = createApp({ render: () => h('div', [h(Tool, { entry, open: true, patchView })]) });
    app.use(createPinia());
    app.mount(host);
    await nextTick();
    await nextTick();
    const row = host.querySelector('.tl-entry');
    const chip = row?.querySelector('.tl-entry-chip');
    const lines = row?.querySelector('.patch-diff-lines');
    return {
      chips: [...(row?.querySelectorAll('.tl-entry-chip') ?? [])].map((node) => node.textContent?.trim() || ''),
      chipTag: chip?.tagName ?? '',
      chipTitle: chip?.getAttribute('title') ?? '',
      summary: row?.querySelector('.tl-entry-summary')?.textContent?.trim() || '',
      maxHeight: lines ? getComputedStyle(lines).maxHeight : '',
      scrolls: lines ? lines.scrollHeight > lines.clientHeight + 1 : false
    };
  });
  // 绝对路径没法在左栏定位，但文件名必须出现在折叠行里。
  expect(result.chips).toEqual(['sample.py']);
  expect(result.chipTag).toBe('SPAN');
  expect(result.chipTitle).toBe('/workspaces/local_user/workspace/temp/sample.py');
  expect(result.summary).toBe('');
  // diff 行区自己滚，不把时间线拉长。
  expect(result.maxHeight).toBe('320px');
  expect(result.scrolls).toBe(true);
});
