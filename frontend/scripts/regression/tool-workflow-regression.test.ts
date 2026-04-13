import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildStructuredToolResultNote,
  buildStructuredToolResultView
} from '../../src/components/chat/toolWorkflowStructuredView';
import { formatWorkflowDetailForDisplay } from '../../src/components/chat/toolWorkflowDetailFormatter';
import {
  buildWorkflowToolRuns,
  resolveWorkflowPendingPlaceholder
} from '../../src/components/chat/toolWorkflowRunModel';
import {
  formatWorkflowConsumedTokensLabel,
  resolveWorkflowConsumedTokens
} from '../../src/components/chat/toolWorkflowUsage';

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

test('tool workflow run model accepts mixed legacy workflow field names during live updates', () => {
  const rows = buildWorkflowToolRuns([
    {
      item_id: 'call-legacy',
      event: 'tool_call',
      tool: 'execute_command',
      tool_call_id: 'tool-legacy',
      command_session_id: 'cmd-legacy',
      status: 'loading',
      detail: '{"command":"pwd"}'
    },
    {
      itemId: 'output-legacy',
      event_type: 'tool_output_delta',
      tool_name: 'execute_command',
      call_id: 'tool-legacy',
      status: 'loading',
      detail: '/workspace'
    },
    {
      id: 'result-legacy',
      event: 'tool_result',
      name: 'execute_command',
      tool_call_id: 'tool-legacy',
      status: 'completed',
      detail: '{"stdout":"/workspace"}'
    }
  ]);
  assert.equal(rows.length, 1);
  assert.equal(rows[0]?.toolName, 'execute_command');
  assert.equal(rows[0]?.callItem?.event, 'tool_call');
  assert.equal(rows[0]?.outputItem?.event_type, 'tool_output_delta');
  assert.equal(rows[0]?.resultItem?.event, 'tool_result');
});

test('workflow pending placeholder detects command session activity before tool rows are formed', () => {
  const items = [
    {
      event: 'command_session_start',
      tool: 'execute_command',
      command_session_id: 'cmd-1',
      status: 'loading'
    }
  ];
  assert.equal(buildWorkflowToolRuns(items).length, 0);
  assert.deepEqual(resolveWorkflowPendingPlaceholder(items), {
    kind: 'tool',
    toolName: 'execute_command',
    eventType: 'command_session_start'
  });
});

test('workflow consumed tokens prefer explicit request consumption over context occupancy', () => {
  const tokens = resolveWorkflowConsumedTokens(
    JSON.stringify({
      result: {
        meta: {
          request_consumed_tokens: 1536,
          context_occupancy_tokens: 8192
        },
        usage: {
          total_tokens: 8192
        }
      }
    })
  );
  assert.equal(tokens, 1536);
  assert.equal(formatWorkflowConsumedTokensLabel(tokens), '1,536 tok');
});

test('workflow consumed tokens fall back to round usage totals when explicit value is absent', () => {
  const tokens = resolveWorkflowConsumedTokens(
    JSON.stringify({
      data: {
        round_usage: {
          total_tokens: 2840
        },
        context_occupancy_tokens: 12064
      }
    })
  );
  assert.equal(tokens, 2840);
});

test('workflow consumed tokens ignore context-only payloads', () => {
  const tokens = resolveWorkflowConsumedTokens(
    JSON.stringify({
      data: {
        context_occupancy_tokens: 12064,
        context_usage: {
          context_tokens: 12064
        }
      }
    })
  );
  assert.equal(tokens, null);
  assert.equal(formatWorkflowConsumedTokensLabel(tokens), '');
});

test('workflow consumed tokens can still read request usage from item payload when detail is observation-only', () => {
  const tokens = resolveWorkflowConsumedTokens(
    '{"summary":"tool finished"}',
    {
      eventType: 'tool_result',
      payload: {
        result: {
          meta: {
            request_consumed_tokens: 2048,
            context_occupancy_tokens: 10000
          }
        }
      }
    }
  );
  assert.equal(tokens, 2048);
  assert.equal(formatWorkflowConsumedTokensLabel(tokens), '2,048 tok');
});
