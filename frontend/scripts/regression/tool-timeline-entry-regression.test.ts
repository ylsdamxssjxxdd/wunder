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
import { buildWorkflowToolRuns } from '../../src/components/chat/toolWorkflowRunModel';
import { buildTimelineStructuredView } from '../../src/components/chat/toolTimelineStructured';
import { extractToolResultDataObject } from '../../src/components/chat/toolWorkflowResultPayload';

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

test('question panels retain questions, options and recommendation through nested result envelopes', () => {
  for (const toolName of ['问询面板', 'question_panel', 'ask_panel']) {
    const run = baseRun({
      toolName,
      resultItem: callItem({ result: { ok: true, data: { action: 'question_panel', state: 'awaiting_input',
        summary: 'Fixture panel summary', data: { questions: [
          { question: 'Choose a fixture option?', multiple: true, options: [
            { label: 'Option A', description: 'Fixture description', recommended: true }, 'Option B'
          ] }
        ] } } } })
    });
    const view = buildTimelineStructuredView(run, t);
    assert.equal(view?.variant, 'question');
    assert.equal(view?.groups[0].title, 'Choose a fixture option?');
    assert.equal(view?.groups[0].rows[0].title, 'chat.toolWorkflow.detail.multipleChoice');
    assert.equal(view?.groups[0].rows[1].body, 'Fixture description');
    assert.equal(view?.groups[0].rows[1].meta, 'chat.inquiry.recommended');
    assert.equal(view?.groups[0].rows[2].title, 'Option B');
    assert.ok(buildTimelineToolEntry(run, t).detailText.includes('Choose a fixture option?'));
  }
});

test('legacy and pending question panels recover the call payload without requiring a result', () => {
  const run = baseRun({ toolName: '问询面板', callItem: callItem({
    args: { question: 'Fixture question?', options: ['Option A', 'Option B'] }
  }, { status: 'running' }) });
  const view = buildTimelineStructuredView(run, t);
  assert.equal(view?.groups[0].title, 'Fixture question?');
  assert.equal(view?.groups[0].rows[1].title, 'Option A');
  assert.equal(buildTimelineToolEntry(run, t).expandable, true);
});

test('read image preserves source metadata beside the processed media result', () => {
  const data = { path: 'fixtures/sample.png', resolved_path: 'hidden-source', media_kind: 'image', size_bytes: 128,
    result: { kind: 'image', name: 'sample.png', frame_count: 1, warnings: ['Fixture warning'],
      attachments: [{ name: 'sample.png', content_type: 'image/png', public_path: 'hidden-resource' }] } };
  // A media result is useful payload, not another envelope to unwrap.
  assert.equal(extractToolResultDataObject({ data })?.path, 'fixtures/sample.png');
  for (const toolName of ['读图工具', 'read_image', 'view_image']) {
    const run = baseRun({ toolName, resultItem: callItem({ result: { data: { action: 'read_image', data } } }) });
    const view = buildTimelineStructuredView(run, t);
    assert.equal(view?.variant, 'media');
    assert.equal(view?.groups[0].rows[0].title, 'fixtures/sample.png');
    assert.match(view!.groups[0].rows[0].body!, /128/);
    assert.match(view!.groups[0].rows[0].body!, /Fixture warning/);
    assert.doesNotMatch(JSON.stringify(view), /hidden-source|hidden-resource|attachments/);
    assert.doesNotMatch(buildTimelineToolEntry(run, t).detailText, /hidden-source|hidden-resource/);
  }
});

test('durable terminal panels recover stored arguments and raw tool results', () => {
  const [run] = buildWorkflowToolRuns([{ id: 'fixture-panel', toolName: '问询面板', eventType: 'tool_result',
    status: 'completed', detail: '{}', toolCallRawDetail: JSON.stringify({
      arguments: JSON.stringify({ question: 'Stored fixture question?', options: ['Option A'] })
    }) }]);
  assert.equal(buildTimelineStructuredView(run, t)?.groups[0].title, 'Stored fixture question?');
  const rawRun = baseRun({ toolName: 'read_file', resultItem: callItem({}, {
    toolResultRawDetail: JSON.stringify({ data: { data: { content: 'Stored fixture content' } } })
  }) });
  assert.equal(buildTimelineToolEntry(rawRun, t).detailText, 'Stored fixture content');
  assert.match(JSON.stringify(buildTimelineStructuredView(rawRun, t)), /Stored fixture content/);
});

test('generic tool text, content blocks and structured payloads stay readable without internal handles', () => {
  for (const data of [
    { data: { text: 'Fixture nested content' } },
    { content: [{ type: 'text', text: 'Fixture block content' }] },
    { structuredContent: { value: 'Fixture structured content', query_handle: 'hidden-handle' } },
    { count: 2, query_handle: 'hidden-handle', resolved_path: 'hidden-source' }
  ]) {
    const entry = buildTimelineToolEntry(baseRun({ toolName: 'fixture_tool',
      resultItem: callItem({ result: { data } }) }), t);
    assert.ok(entry.detailText);
    assert.doesNotMatch(entry.detailText, /hidden-handle|hidden-source/);
    assert.equal(entry.summary, '');
  }
});

test('panel and plan projections cap item counts and text length', () => {
  const question = { question: 'q'.repeat(2000), options: Array(30).fill('o'.repeat(2000)) };
  const view = buildTimelineStructuredView(baseRun({ toolName: 'question_panel',
    resultItem: callItem({ data: { questions: Array(30).fill(question) } }) }), t)!;
  assert.equal(view.groups.length, 4);
  assert.equal(view.groups[0].rows.length, 5);
  assert.equal(view.groups[0].title!.length, 801);
  const plan = buildTimelineStructuredView(baseRun({ toolName: '计划面板',
    callItem: callItem({ args: { plan: Array(30).fill({ step: 'Fixture step', status: 'in_progress' }) } }),
    resultItem: callItem({ data: { status: 'ok' } }) }), t)!;
  assert.equal(plan.variant, 'plan');
  assert.equal(plan.groups[0].rows.length, 12);
  assert.equal(plan.groups[0].rows[0].meta, 'chat.toolWorkflow.statusRunning');
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
