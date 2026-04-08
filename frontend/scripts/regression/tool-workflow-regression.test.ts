import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildStructuredToolResultNote,
  buildStructuredToolResultView
} from '../../src/components/chat/toolWorkflowStructuredView';
import { formatWorkflowDetailForDisplay } from '../../src/components/chat/toolWorkflowDetailFormatter';
import { buildWorkflowToolRuns } from '../../src/components/chat/toolWorkflowRunModel';

const messages: Record<string, string> = {
  'chat.toolWorkflow.detail.hits': 'Hits',
  'chat.toolWorkflow.detail.scannedFiles': 'Scanned files',
  'chat.toolWorkflow.detail.bytes': 'Bytes'
};

const t = (key: string): string => messages[key] || key;

test('search structured view keeps local-only guidance when there are zero hits', () => {
  const data = {
    returned_match_count: 0,
    scanned_files: 3,
    scope_note: 'Searches local workspace files only.',
    summary: {
      next_hint: 'Use list_files first.'
    }
  };
  const view = buildStructuredToolResultView('search_content', null, data, t);
  assert.ok(view);
  assert.equal(view?.variant, 'search');
  assert.deepEqual(
    view?.metrics.map((item) => [item.key, item.value]),
    [
      ['hits', '0'],
      ['scanned', '3']
    ]
  );
  const rowTitles = view?.groups.flatMap((group) => group.rows.map((row) => row.title)) || [];
  assert.ok(rowTitles.includes('Searches local workspace files only.'));
  assert.ok(rowTitles.includes('Use list_files first.'));
  assert.equal(
    buildStructuredToolResultNote('search_content', null, data, t),
    'Scanned files 3'
  );
});

test('write structured view reuses call content as result preview', () => {
  const data = {
    path: './notes/todo.md',
    bytes: 19
  };
  const view = buildStructuredToolResultView(
    'write_file',
    null,
    data,
    t,
    {
      path: './notes/todo.md',
      content: '# Todo\n- one\n- two'
    }
  );
  assert.ok(view);
  assert.equal(view?.variant, 'write');
  assert.deepEqual(
    view?.metrics.map((item) => [item.key, item.value]),
    [['bytes', '19']]
  );
  const row = view?.groups[0]?.rows[0];
  assert.equal(row?.title, './notes/todo.md');
  assert.equal(row?.body, '# Todo\n- one\n- two');
});

test('tool detail formatter pretty prints valid JSON', () => {
  const raw = '{"summary":"ok","count":2}';
  assert.equal(
    formatWorkflowDetailForDisplay(raw),
    '{\n  "summary": "ok",\n  "count": 2\n}'
  );
});

test('tool detail formatter converts JSONL into readable JSON array', () => {
  const raw = '{"step":1,"status":"ok"}\n{"step":2,"status":"done"}';
  assert.equal(
    formatWorkflowDetailForDisplay(raw),
    '[\n  {\n    "step": 1,\n    "status": "ok"\n  },\n  {\n    "step": 2,\n    "status": "done"\n  }\n]'
  );
});

test('tool detail formatter keeps plain text when detail is not JSON', () => {
  const raw = 'tool result text';
  assert.equal(formatWorkflowDetailForDisplay(raw), raw);
});

test('tool workflow run model creates a live row from tool_call before final reply', () => {
  const rows = buildWorkflowToolRuns([
    {
      id: 'call-1',
      eventType: 'tool_call',
      toolName: 'execute_command',
      toolCallId: 'tool-1',
      status: 'loading',
      detail: '{"command":"pwd"}'
    }
  ]);
  assert.equal(rows.length, 1);
  assert.equal(rows[0]?.toolName, 'execute_command');
  assert.equal(rows[0]?.callItem?.eventType, 'tool_call');
  assert.equal(rows[0]?.resultItem, null);
});

test('tool workflow run model keeps mid-run output and final result on the same row', () => {
  const rows = buildWorkflowToolRuns([
    {
      id: 'call-1',
      eventType: 'tool_call',
      toolName: 'execute_command',
      toolCallId: 'tool-1',
      status: 'loading',
      detail: '{"command":"pwd"}'
    },
    {
      id: 'output-1',
      eventType: 'tool_output_delta',
      toolName: 'execute_command',
      toolCallId: 'tool-1',
      status: 'loading',
      detail: '/workspace'
    },
    {
      id: 'result-1',
      eventType: 'tool_result',
      toolName: 'execute_command',
      toolCallId: 'tool-1',
      status: 'completed',
      detail: '{"stdout":"/workspace"}'
    }
  ]);
  assert.equal(rows.length, 1);
  assert.equal(rows[0]?.callItem?.eventType, 'tool_call');
  assert.equal(rows[0]?.outputItem?.eventType, 'tool_output_delta');
  assert.equal(rows[0]?.resultItem?.eventType, 'tool_result');
});
