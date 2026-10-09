// AI生成 · 时间线工具条目回归：错误/输出去重、参数化摘要、编辑类 diff 卡片。
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildTimelineToolEntry,
  type TimelineToolEntry
} from '../../src/components/chat/toolTimelineModel';
import {
  buildTimelineEditPatchView,
  buildTimelinePatchView,
  isTimelineEditToolRun
} from '../../src/components/chat/toolTimelinePatch';
import type { RawToolRun, WorkflowItem } from '../../src/components/chat/toolWorkflowRunModel';

const t = (key: string, params?: Record<string, unknown>): string =>
  params && params.line !== undefined ? `${key}:${params.line}` : key;

const callItem = (detail: unknown, extra: Partial<WorkflowItem> = {}): WorkflowItem => ({
  detail: typeof detail === 'string' ? detail : JSON.stringify(detail),
  ...extra
});

const baseRun = (overrides: Partial<RawToolRun> = {}): RawToolRun => ({
  key: 'run-1',
  toolName: 'execute_command',
  toolDisplayName: '执行命令',
  toolRuntimeName: 'execute_command',
  toolFunctionName: '',
  callItem: null,
  outputItem: null,
  resultItem: null,
  ...overrides
});

const TRACEBACK = [
  'Traceback (most recent call last):',
  '  File "draw.py", line 18, in <module>',
  '    path = Path(np.vstack([verts, verts[0]]))',
  "ValueError: 'codes' must be a 1D list or array"
].join('\n');

test('failed command keeps one error surface: duplicated stderr is dropped from the output body', () => {
  const run = baseRun({
    callItem: callItem({ args: { content: 'python draw.py' } }),
    resultItem: callItem({
      data: { exit_code: 1, stderr: TRACEBACK }
    })
  });
  const entry: TimelineToolEntry = buildTimelineToolEntry(run, t);

  assert.equal(entry.status, 'failed');
  // 摘要显示调用参数（命令），不再塞错误堆栈。
  assert.equal(entry.summary, 'python draw.py');
  assert.equal(entry.summaryMono, true);
  // stderr 同时是错误摘要与输出正文：输出块去重丢弃。
  assert.ok(entry.errorText.includes("ValueError: 'codes' must be a 1D list or array"));
  assert.equal(entry.detailText, '');
  assert.equal(entry.exitCode, 1);
  assert.equal(entry.expandable, true);
});

test('failed command keeps distinct stdout output alongside the error block', () => {
  const run = baseRun({
    callItem: callItem({ args: { content: 'python build.py' } }),
    resultItem: callItem({
      data: {
        exit_code: 1,
        stdout: 'partial build output',
        error: 'TOOL_EXEC_NON_ZERO_EXIT',
        stderr: 'real traceback body'
      }
    })
  });
  const entry = buildTimelineToolEntry(run, t);

  assert.equal(entry.status, 'failed');
  assert.equal(entry.errorText, 'TOOL_EXEC_NON_ZERO_EXIT');
  // stdout 与错误摘要内容不同：输出块保留。
  assert.equal(entry.detailText, 'partial build output');
  assert.equal(entry.exitCode, 1);
});

test('error head that is a truncated copy of the output does not render twice', () => {
  const longStderr = `${TRACEBACK}\n${'x'.repeat(600)}`;
  const run = baseRun({
    callItem: callItem({ args: { content: 'python draw.py' } }),
    resultItem: callItem({
      data: {
        exit_code: 1,
        error_detail_head: longStderr.slice(0, 500),
        stderr: longStderr
      }
    })
  });
  const entry = buildTimelineToolEntry(run, t);

  assert.equal(entry.status, 'failed');
  assert.ok(entry.errorText.length > 160);
  // 错误摘要是输出的前缀拷贝（接近全文），输出块丢弃。
  assert.equal(entry.detailText, '');
});

test('tool results never leak into the collapsed summary', () => {
  const leaked = buildTimelineToolEntry(baseRun({
    toolName: 'execute_command',
    callItem: null,
    resultItem: callItem({
      data: { output: 'RESULT CONTENT SHOULD NOT APPEAR' }
    })
  }), t);
  assert.equal(leaked.summary, '');

  const query = buildTimelineToolEntry(baseRun({
    toolName: 'search_content',
    toolDisplayName: '搜索内容',
    toolRuntimeName: 'search_content',
    callItem: callItem({ args: { query: 'timeline dedup' } }),
    resultItem: callItem({
      data: { query: 'RESULT QUERY LEAK', matches: [] }
    })
  }), t);
  assert.equal(query.summary, 'timeline dedup');
});

test('text_edit replace folds into a red/green diff card with +/- stats', () => {
  const run = baseRun({
    toolName: 'text_edit',
    toolDisplayName: '文本编辑',
    toolRuntimeName: 'text_edit',
    callItem: callItem({
      args: {
        file_path: 'src/a.rs',
        old_string: 'const A = 1;',
        new_string: 'const A = 2;\nconst B = 3;'
      }
    })
  });
  const view = buildTimelineEditPatchView(run, t);
  assert.ok(view, 'replace edit should build a patch view');
  assert.equal(view.files.length, 1);

  const file = view.files[0];
  assert.equal(file.title, 'src/a.rs');
  assert.equal(file.meta, 'chat.timeline.patch.actionUpdate');
  const kinds = file.lines.map((line) => line.kind);
  assert.deepEqual(kinds, ['delete', 'add', 'add']);
  assert.equal(file.lines[0].text, 'const A = 1;');
  assert.equal(file.lines[1].text, 'const A = 2;');
  assert.equal(file.lines[2].text, 'const B = 3;');
  assert.equal(view.metrics[1].value, '2');
  assert.equal(view.metrics[2].value, '1');

  // 与 apply_patch 走同一个入口。
  assert.deepEqual(buildTimelinePatchView(run, t), view);
});

test('str_replace_editor create/insert map to add-only diffs with insert line numbers', () => {
  const create = buildTimelineEditPatchView(baseRun({
    toolName: 'str_replace_editor',
    toolRuntimeName: 'str_replace_editor',
    callItem: callItem({
      args: { command: 'create', path: 'docs/new.md', file_text: '# title\nbody' }
    })
  }), t);
  assert.ok(create);
  assert.equal(create.files[0].meta, 'chat.timeline.patch.actionAdd');
  assert.deepEqual(create.files[0].lines.map((line) => line.kind), ['add', 'add']);

  const insert = buildTimelineEditPatchView(baseRun({
    toolName: '文本编辑',
    toolRuntimeName: 'text_edit',
    callItem: callItem({
      args: { command: 'insert', path: 'docs/new.md', new_str: 'inserted', insert_line: 5 }
    })
  }), t);
  assert.ok(insert);
  assert.equal(insert.files[0].meta, `chat.timeline.patch.actionInsert · ${t('chat.timeline.patch.insertAfterLine', { line: 5 })}`);
  const inserted = insert.files[0].lines[0];
  assert.equal(inserted.kind, 'add');
  assert.equal(inserted.newLine, 6);
});

test('write_file content folds into an add-only diff and view command stays plain output', () => {
  const write = buildTimelineEditPatchView(baseRun({
    toolName: 'write_file',
    toolDisplayName: '写入文件',
    toolRuntimeName: 'write_file',
    callItem: callItem({ args: { path: 'docs/x.md', content: '# hi\n' } })
  }), t);
  assert.ok(write);
  assert.equal(write.files[0].meta, 'chat.timeline.patch.actionWrite');
  assert.deepEqual(write.files[0].lines.map((line) => [line.kind, line.text]), [['add', '# hi']]);

  const view = buildTimelineEditPatchView(baseRun({
    toolName: 'str_replace_editor',
    toolRuntimeName: 'str_replace_editor',
    callItem: callItem({ args: { command: 'view', path: 'docs/x.md' } })
  }), t);
  assert.equal(view, null);

  const unrelated = buildTimelineEditPatchView(baseRun({
    toolName: 'web_search',
    toolRuntimeName: 'web_search',
    callItem: callItem({ args: { query: 'x' } })
  }), t);
  assert.equal(unrelated, null);
});

test('replace_all and identical no-op edits are handled explicitly', () => {
  const replaceAll = buildTimelineEditPatchView(baseRun({
    toolName: 'text_edit',
    toolRuntimeName: 'text_edit',
    callItem: callItem({
      args: { file_path: 'a.txt', old_string: 'x', new_string: 'y', replace_all: true }
    })
  }), t);
  assert.ok(replaceAll);
  assert.equal(replaceAll.files[0].meta, `chat.timeline.patch.actionUpdate · chat.timeline.patch.replaceAll`);

  const noop = buildTimelineEditPatchView(baseRun({
    toolName: 'text_edit',
    toolRuntimeName: 'text_edit',
    callItem: callItem({
      args: { file_path: 'a.txt', old_string: 'same', new_string: 'same' }
    })
  }), t);
  assert.equal(noop, null);
});

test('edit tool detection covers runtime and display name aliases', () => {
  assert.equal(isTimelineEditToolRun(baseRun({ toolName: 'text_edit' })), true);
  assert.equal(isTimelineEditToolRun(baseRun({ toolName: 'unknown', toolDisplayName: '文本编辑' })), true);
  assert.equal(isTimelineEditToolRun(baseRun({ toolName: 'unknown', toolDisplayName: '写入文件' })), true);
  assert.equal(isTimelineEditToolRun(baseRun({ toolName: 'execute_command' })), false);
});
