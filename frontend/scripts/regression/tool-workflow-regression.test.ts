import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import {
  buildStructuredToolResultNote,
  buildStructuredToolResultView
} from '../../src/components/chat/toolWorkflowStructuredView';
import { extractToolResultDataObject } from '../../src/components/chat/toolWorkflowResultPayload';
import { formatWorkflowDetailForDisplay } from '../../src/components/chat/toolWorkflowDetailFormatter';
import {
  buildWorkflowToolRuns,
  resolveWorkflowPendingPlaceholder
} from '../../src/components/chat/toolWorkflowRunModel';
import {
  formatWorkflowContextTokensLabel,
  formatWorkflowConsumedTokensLabel,
  resolveWorkflowContextTokens,
  resolveWorkflowEntryContextTokenResolution,
  resolveWorkflowEntryConsumedTokenResolution,
  resolveWorkflowEntryConsumedTokens,
  resolveWorkflowConsumedTokens
} from '../../src/components/chat/toolWorkflowUsage';
import {
  formatWorkflowDurationLabel,
  resolveWorkflowDurationMs,
  resolveWorkflowEntryDurationMs
} from '../../src/utils/toolWorkflowTiming';

const messages: Record<string, string> = {
  'chat.toolWorkflow.detail.hits': 'Hits',
  'chat.toolWorkflow.detail.hit': 'Hit',
  'chat.toolWorkflow.detail.scannedFiles': 'Scanned files',
  'chat.toolWorkflow.detail.bytes': 'Bytes',
  'chat.toolWorkflow.detail.rows': 'Rows',
  'chat.toolWorkflow.detail.row': 'Row',
  'chat.toolWorkflow.detail.columns': 'Columns',
  'chat.toolWorkflow.detail.table': 'Table',
  'chat.toolWorkflow.detail.sql': 'SQL',
  'chat.toolWorkflow.detail.elapsed': 'Elapsed',
  'chat.toolWorkflow.detail.truncated': 'Truncated',
  'chat.toolWorkflow.detail.query': 'Query',
  'chat.toolWorkflow.detail.documents': 'Documents',
  'chat.toolWorkflow.detail.document': 'Document',
  'common.yes': 'Yes'
};

const t = (key: string): string => messages[key] || key;

const frontendRoot = resolve(process.cwd());
const readSource = (relativePath: string): string =>
  readFileSync(resolve(frontendRoot, relativePath), 'utf8').replace(/\r\n/g, '\n');

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

test('write structured view only renders written content', () => {
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
  assert.deepEqual(view?.metrics, []);
  const row = view?.groups[0]?.rows[0];
  assert.equal(row?.title, '');
  assert.equal(row?.body, '# Todo\n- one\n- two');
  assert.equal(JSON.stringify(view).includes('./notes/todo.md'), false);
  assert.equal(JSON.stringify(view).includes('19'), false);
  assert.equal(buildStructuredToolResultNote('write_file', null, data, t), '');
});

test('read structured view only renders file content', () => {
  const data = {
    content: '>>> ./notes/todo.md\n# Todo\n- one\n- two',
    meta: {
      files: [
        {
          path: './notes/todo.md',
          read_lines: 3,
          total_lines: 9
        }
      ]
    }
  };
  const view = buildStructuredToolResultView('read_file', null, data, t);
  assert.ok(view);
  assert.equal(view?.variant, 'read');
  assert.deepEqual(view?.metrics, []);
  const row = view?.groups[0]?.rows[0];
  assert.equal(row?.title, '');
  assert.equal(row?.meta, undefined);
  assert.equal(row?.body, '# Todo\n- one\n- two');
  const serialized = JSON.stringify(view);
  assert.equal(serialized.includes('./notes/todo.md'), false);
  assert.equal(serialized.includes('read_lines'), false);
});

test('read structured view unwraps nested tool data envelopes', () => {
  const content = '>>> ./sample.py\n1: print("ok")\n2: done';
  const resultObject = {
    data: {
      action: 'read_file',
      data: {
        budget_file_limit_hit: false,
        content
      }
    }
  };
  const data = extractToolResultDataObject(resultObject);
  assert.equal(data?.content, content);
  assert.equal(data?.action, undefined);

  const view = buildStructuredToolResultView('read_file', null, resultObject, t);
  assert.ok(view);
  assert.equal(view?.variant, 'read');
  const row = view?.groups[0]?.rows[0];
  assert.equal(row?.title, '');
  assert.equal(row?.body, '1: print("ok")\n2: done');
  const serialized = JSON.stringify(view);
  assert.equal(serialized.includes('budget_file_limit_hit'), false);
  assert.equal(serialized.includes('action'), false);
  assert.equal(serialized.includes('data'), false);
  assert.equal(serialized.includes('>>> ./sample.py'), false);
});

test('workflow result section prefers file content and model observation before raw event detail', () => {
  const source = readSource('src/components/chat/MessageToolWorkflow.vue');
  const readBranch = source.indexOf('if (isReadFileTool(entry.toolName))');
  const writeBranch = source.indexOf('if (isWriteFileTool(entry.toolName))', readBranch);
  const commandBranch = source.indexOf('if (isCommandStreamVisualizationEnabled() && isExecuteCommandTool(entry.toolName))', writeBranch);
  const observationBranch = source.indexOf('const observation = buildTextPreview', commandBranch);
  const rawDetailBranch = source.indexOf('if (rawResultDetail)', observationBranch);
  assert.ok(readBranch >= 0);
  assert.ok(writeBranch > readBranch);
  assert.ok(commandBranch > writeBranch);
  assert.ok(observationBranch > commandBranch);
  assert.ok(rawDetailBranch > observationBranch);
  assert.ok(source.includes('pickObservationText(...detailObjects, dataObject, resultObject)'));
  assert.ok(source.includes('formatToolObservationText(pickObservationText(...detailObjects, dataObject, resultObject))'));
  assert.ok(source.includes('pickFileContentText(extractCallArgs(entry.callItem), ...detailObjects, dataObject, resultObject)'));
});

test('read file workflow summary can recover filename from call args and read result content headers', () => {
  const source = readSource('src/components/chat/MessageToolWorkflow.vue');
  const collectResultStart = source.indexOf('const collectPathHintsFromResult =');
  assert.ok(collectResultStart >= 0);
  const collectResultEnd = source.indexOf('const splitMoveText =', collectResultStart);
  assert.ok(collectResultEnd > collectResultStart);
  const collectResultSource = source.slice(collectResultStart, collectResultEnd);
  assert.ok(collectResultSource.includes('Array.isArray(meta?.files)'));
  assert.ok(collectResultSource.includes('parseReadFileSections(content)'));
  assert.ok(collectResultSource.includes('appendPathCandidate(hints, section.path)'));

  const readTitleStart = source.indexOf('const resolveReadFileSummaryTitle =');
  assert.ok(readTitleStart >= 0);
  const readTitleEnd = source.indexOf('const resolveWriteFileSummaryTitle =', readTitleStart);
  assert.ok(readTitleEnd > readTitleStart);
  const readTitleSource = source.slice(readTitleStart, readTitleEnd);
  assert.ok(readTitleSource.includes('collectReadTargetLabels(args)'));
  assert.ok(readTitleSource.includes('readTargets.length > 0 ? readTargets : pathHints'));
});

test('database structured view shows rows without surfacing query_handle', () => {
  const data = {
    columns_jsonl: 'total_count',
    columns_count: 1,
    rows_count: 1,
    rows_jsonl: '{"total_count":50000}',
    elapsed_ms: 40.82,
    query_handle: 'opaque-handle',
    truncated: false
  };
  const view = buildStructuredToolResultView(
    'extra_mcp@db_query_company_all_personnel',
    null,
    data,
    t,
    {
      sql: 'SELECT COUNT(*) AS total_count FROM `example_table`'
    }
  );
  assert.ok(view);
  assert.equal(view?.variant, 'database');
  const serialized = JSON.stringify(view);
  assert.ok(serialized.includes('total_count: 50000'));
  assert.ok(!serialized.includes('query_handle'));
  assert.ok(!serialized.includes('opaque-handle'));
});

test('knowledge structured view shows query and retrieved chunks', () => {
  const data = {
    total: 1,
    chunks: [
      {
        document_name: 'reference.md',
        content: 'A compact retrieval chunk.',
        similarity: 0.88
      }
    ],
    documents: [
      {
        name: 'reference.md',
        count: 1
      }
    ],
    elapsed_ms: 18
  };
  const view = buildStructuredToolResultView(
    'extra_mcp@kb_query_product_docs',
    null,
    data,
    t,
    {
      query: 'headcount count'
    }
  );
  assert.ok(view);
  assert.equal(view?.variant, 'knowledge');
  const serialized = JSON.stringify(view);
  assert.ok(serialized.includes('headcount count'));
  assert.ok(serialized.includes('A compact retrieval chunk.'));
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

test('tool workflow run model creates the first row from runtime tool_call_started', () => {
  const rows = buildWorkflowToolRuns([
    {
      id: 'runtime-call-1',
      event_type: 'tool_call_started',
      tool_name: 'read_file',
      tool_call_id: 'tool-runtime-1',
      status: 'loading',
      detail: '{"path":"./sample.txt"}'
    }
  ]);
  assert.equal(rows.length, 1);
  assert.equal(rows[0]?.toolName, 'read_file');
  assert.equal(rows[0]?.callItem?.event_type, 'tool_call_started');
  assert.equal(rows[0]?.resultItem, null);
  assert.deepEqual(resolveWorkflowPendingPlaceholder(rows.map((row) => row.callItem).filter(Boolean)), {
    kind: 'tool',
    toolName: 'read_file',
    toolDisplayName: '',
    toolRuntimeName: 'read_file',
    toolFunctionName: '',
    eventType: 'tool_call_started'
  });
});

test('tool workflow run model merges runtime started and completed events', () => {
  const rows = buildWorkflowToolRuns([
    {
      id: 'runtime-call-1',
      event_type: 'tool_call_started',
      tool_name: 'read_file',
      tool_call_id: 'tool-runtime-1',
      status: 'loading',
      detail: '{"path":"./sample.txt"}'
    },
    {
      id: 'runtime-result-1',
      event_type: 'tool_call_completed',
      tool_name: 'read_file',
      tool_call_id: 'tool-runtime-1',
      status: 'completed',
      detail: '{"data":{"content":">>> ./sample.txt\\n1: ok"}}'
    }
  ]);
  assert.equal(rows.length, 1);
  assert.equal(rows[0]?.callItem?.event_type, 'tool_call_started');
  assert.equal(rows[0]?.resultItem?.event_type, 'tool_call_completed');
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

test('tool workflow run model preserves friendly tool identity from MCP events', () => {
  const rows = buildWorkflowToolRuns([
    {
      id: 'call-friendly',
      eventType: 'tool_call',
      toolName: 'extra_mcp@kb_query_product_docs',
      toolDisplayName: '知识库检索（产品文档）',
      toolRuntimeName: 'extra_mcp@kb_query_product_docs',
      toolFunctionName: 'tool_2d9e84',
      toolCallId: 'tool-friendly',
      status: 'loading',
      detail: '{"args":{"query":"count","limit":5}}'
    },
    {
      id: 'result-friendly',
      eventType: 'tool_result',
      toolName: 'extra_mcp@kb_query_product_docs',
      tool_display_name: '知识库检索（产品文档）',
      tool_runtime_name: 'extra_mcp@kb_query_product_docs',
      tool_function_name: 'tool_2d9e84',
      tool_call_id: 'tool-friendly',
      status: 'completed',
      detail: '{"data":{"text":"ok"}}'
    }
  ]);
  assert.equal(rows.length, 1);
  assert.equal(rows[0]?.toolName, 'extra_mcp@kb_query_product_docs');
  assert.equal(rows[0]?.toolDisplayName, '知识库检索（产品文档）');
  assert.equal(rows[0]?.toolRuntimeName, 'extra_mcp@kb_query_product_docs');
  assert.equal(rows[0]?.toolFunctionName, 'tool_2d9e84');
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

test('tool workflow run model merges refreshed command session events with the original tool call', () => {
  const commandArgs = {
    tool: 'execute_command',
    arguments: {
      content: 'sample command',
      timeout_s: 35
    }
  };
  const rows = buildWorkflowToolRuns([
    {
      id: 'tool-call-1',
      eventType: 'tool_call',
      toolName: 'execute_command',
      toolCallId: 'tool-1',
      status: 'loading',
      detail: JSON.stringify(commandArgs)
    },
    {
      id: 'cmd-start-1',
      eventType: 'command_session_start',
      toolName: 'execute_command',
      toolCallId: 'tool-1',
      commandSessionId: 'cmd-1',
      status: 'loading',
      detail: JSON.stringify({
        command_session_id: 'cmd-1',
        tool_call_id: 'tool-1',
        command: 'sample command'
      })
    },
    {
      id: 'cmd-summary-1',
      eventType: 'command_session_summary',
      toolName: 'execute_command',
      toolCallId: 'tool-1',
      commandSessionId: 'cmd-1',
      status: 'completed',
      detail: JSON.stringify({
        command_session_id: 'cmd-1',
        tool_call_id: 'tool-1',
        command: 'sample command',
        exit_code: 0
      })
    }
  ]);

  assert.equal(rows.length, 1);
  assert.equal(rows[0]?.callItem?.eventType, 'tool_call');
  assert.equal(rows[0]?.resultItem?.eventType, 'command_session_summary');
  assert.equal(rows[0]?.callItem?.detail, JSON.stringify(commandArgs));
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
  assert.equal(buildWorkflowToolRuns(items).length, 1);
  assert.deepEqual(resolveWorkflowPendingPlaceholder(items), {
    kind: 'tool',
    toolName: 'execute_command',
    toolDisplayName: '',
    toolRuntimeName: 'execute_command',
    toolFunctionName: '',
    eventType: 'command_session_start'
  });
});

test('workflow pending placeholder exposes friendly MCP display name', () => {
  const items = [
    {
      event: 'tool_call',
      tool: 'extra_mcp@db_query_company_all_personnel',
      tool_display_name: '数据库查询（人员信息）',
      tool_runtime_name: 'extra_mcp@db_query_company_all_personnel',
      tool_function_name: 'tool_1b9ae7',
      tool_call_id: 'call-db',
      status: 'loading'
    }
  ];
  assert.deepEqual(resolveWorkflowPendingPlaceholder(items), {
    kind: 'tool',
    toolName: 'extra_mcp@db_query_company_all_personnel',
    toolDisplayName: '数据库查询（人员信息）',
    toolRuntimeName: 'extra_mcp@db_query_company_all_personnel',
    toolFunctionName: 'tool_1b9ae7',
    eventType: 'tool_call'
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
  assert.equal(formatWorkflowConsumedTokensLabel(tokens), '1536 token');
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

test('workflow consumed tokens can read nested model usage totals', () => {
  const tokens = resolveWorkflowConsumedTokens(
    JSON.stringify({
      payload: {
        usage: {
          total_tokens: 8066
        }
      }
    })
  );
  assert.equal(tokens, 8066);
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

test('workflow context tokens read context-only payloads for row occupancy', () => {
  const resolved = resolveWorkflowContextTokens(
    JSON.stringify({
      data: {
        context_occupancy_tokens: 12064,
        context_usage: {
          context_tokens: 12064,
          context_max_tokens: 32768
        }
      }
    })
  );
  assert.deepEqual(resolved, {
    tokens: 12064,
    totalTokens: 32768
  });
  assert.equal(formatWorkflowContextTokensLabel(resolved.tokens), '12064 token');
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
  assert.equal(formatWorkflowConsumedTokensLabel(tokens), '2048 token');
});

test('workflow entry consumed tokens prefer tool call model usage over result aggregate', () => {
  const tokens = resolveWorkflowEntryConsumedTokens({
    callItem: {
      eventType: 'tool_call',
      modelRound: 2,
      payload: {
        usage: {
          total_tokens: 512
        },
        context_occupancy_tokens: 4096
      }
    },
    resultItem: {
      eventType: 'tool_result',
      payload: {
        result: {
          meta: {
            request_consumed_tokens: 1536,
            context_occupancy_tokens: 8192
          }
        }
      }
    }
  });
  assert.equal(tokens, 512);
  assert.equal(formatWorkflowConsumedTokensLabel(tokens), '512 token');
});

test('workflow duration resolves nested elapsed timing from tool payload', () => {
  const durationMs = resolveWorkflowDurationMs(
    JSON.stringify({
      result: {
        meta: {
          search: {
            elapsed_ms: 612
          }
        }
      }
    })
  );
  assert.equal(durationMs, 612);
  assert.equal(formatWorkflowDurationLabel(durationMs), '612ms');
});

test('workflow duration ignores byte counts in generated media payloads', () => {
  const durationMs = resolveWorkflowDurationMs({
    tool: 'generate_speech',
    ok: true,
    data: {
      path: '/workspaces/demo/generated_media/speech.wav',
      bytes: 72631,
      voice_clone: true
    }
  });
  assert.equal(durationMs, null);
  assert.equal(formatWorkflowDurationLabel(durationMs), '');
});

test('workflow entry duration prefers explicit result timing when detail is observation only', () => {
  const durationMs = resolveWorkflowEntryDurationMs({
    resultItem: {
      eventType: 'tool_result',
      detail: '{"summary":"tool finished"}',
      durationMs: 1700
    }
  });
  assert.equal(durationMs, 1700);
  assert.equal(formatWorkflowDurationLabel(durationMs), '1.7s');
});

test('workflow entry consumed tokens fall back to result usage when tool call usage is absent', () => {
  const tokens = resolveWorkflowEntryConsumedTokens({
    callItem: {
      eventType: 'tool_call',
      payload: {
        context_occupancy_tokens: 4096
      }
    },
    resultItem: {
      eventType: 'tool_result',
      payload: {
        result: {
          meta: {
            request_consumed_tokens: 768
          }
        }
      }
    }
  });
  assert.equal(tokens, 768);
});

test('workflow entry consumed token resolution records call as the winning source', () => {
  const resolution = resolveWorkflowEntryConsumedTokenResolution({
    callItem: {
      eventType: 'tool_call',
      payload: {
        usage: {
          total_tokens: 321
        }
      }
    },
    resultItem: {
      eventType: 'tool_result',
      payload: {
        result: {
          meta: {
            request_consumed_tokens: 999
          }
        }
      }
    }
  });
  assert.deepEqual(resolution, {
    tokens: 321,
    source: 'call'
  });
});

test('workflow entry consumed token resolution records none when no usage is available', () => {
  const resolution = resolveWorkflowEntryConsumedTokenResolution({
    callItem: {
      eventType: 'tool_call',
      payload: {
        context_occupancy_tokens: 4096
      }
    },
    resultItem: {
      eventType: 'tool_result',
      payload: {
        result: {
          meta: {
            context_occupancy_tokens: 8192
          }
        }
      }
    }
  });
  assert.deepEqual(resolution, {
    tokens: null,
    source: 'none'
  });
});

test('workflow entry context token resolution prefers call-time occupancy', () => {
  const resolution = resolveWorkflowEntryContextTokenResolution({
    callItem: {
      eventType: 'tool_call',
      payload: {
        context_occupancy_tokens: 4096,
        context_total_tokens: 32768
      }
    },
    outputItem: {
      eventType: 'tool_output_delta',
      payload: {
        context_usage: {
          context_tokens: 6144,
          context_max_tokens: 32768
        }
      }
    },
    resultItem: {
      eventType: 'tool_result',
      payload: {
        result: {
          meta: {
            context_occupancy_tokens: 8192
          }
        }
      }
    }
  });
  assert.deepEqual(resolution, {
    tokens: 4096,
    totalTokens: 32768,
    source: 'call'
  });
});

test('workflow entry consumed tokens ignore result usage totals without explicit per-tool consumption', () => {
  const resolution = resolveWorkflowEntryConsumedTokenResolution({
    callItem: {
      eventType: 'tool_call',
      payload: {
        context_occupancy_tokens: 4096
      }
    },
    resultItem: {
      eventType: 'tool_result',
      payload: {
        result: {
          usage: {
            total_tokens: 21946
          },
          meta: {
            context_occupancy_tokens: 21946
          }
        }
      }
    }
  });
  assert.deepEqual(resolution, {
    tokens: null,
    source: 'none'
  });
});
