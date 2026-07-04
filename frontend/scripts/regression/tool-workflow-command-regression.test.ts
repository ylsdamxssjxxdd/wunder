import test from 'node:test';
import assert from 'node:assert/strict';

import { buildCommandCardView } from '../../src/components/chat/toolWorkflowActionViews';
import { buildWorkflowToolCallDebugText } from '../../src/components/chat/toolWorkflowCallDebug';

const messages: Record<string, string> = {
  'chat.toolWorkflow.detail.command': 'Command',
  'chat.toolWorkflow.detail.commands': 'Commands',
  'chat.toolWorkflow.detail.workdir': 'Workdir',
  'chat.toolWorkflow.detail.timeout': 'Timeout',
  'chat.toolWorkflow.detail.exitCode': 'Exit code',
  'chat.toolWorkflow.detail.truncatedCommands': 'Truncated commands',
  'chat.toolWorkflow.detail.totalBytes': 'Total bytes',
  'chat.toolWorkflow.detail.omittedBytes': 'Omitted bytes'
};

const t = (key: string): string => messages[key] || key;

test('empty command card does not synthesize placeholder command text', () => {
  const view = buildCommandCardView(
    {
      command: '',
      shell: '',
      exitCode: null,
      stdout: '',
      stderr: '',
      preview: '',
      workdir: '',
      timeout: '',
      commandCount: 0,
      truncatedCommands: null,
      totalBytes: '',
      omittedBytes: '',
      errorText: '',
      showExitCode: false
    },
    t
  );
  assert.equal(view.command, '');
  assert.equal(view.terminalText, '');
  assert.equal(view.previewBody, '');
});

test('command tool call debug text excludes runtime session snapshot fields', () => {
  const debugText = buildWorkflowToolCallDebugText({
    key: 'cmd-1',
    toolName: 'execute_command',
    toolDisplayName: '执行命令',
    toolRuntimeName: 'execute_command',
    toolFunctionName: 'execute_command',
    callItem: {
      id: 'call-1',
      eventType: 'tool_call',
      toolName: 'execute_command',
      toolCallId: 'call-1',
      commandSessionId: 'cmd-1',
      detail: JSON.stringify({
        command: 'npm run build 2>&1',
        command_index: 0,
        command_session_id: 'cmd-1',
        cwd: 'C:\\workspace',
        exit_code: 0,
        started_at: '2026-05-20T00:00:00Z',
        status: 'exited',
        stdout_tail: 'built'
      })
    },
    outputItem: null,
    resultItem: null
  });

  assert.equal(
    debugText,
    JSON.stringify(
      {
        tool: 'execute_command',
        arguments: {
          content: 'npm run build 2>&1'
        }
      },
      null,
      2
    )
  );
  assert.ok(!debugText.includes('stdout_tail'));
  assert.ok(!debugText.includes('exit_code'));
  assert.ok(!debugText.includes('command_session_id'));
});

test('command tool call debug text prefers saved original model call over later runtime detail', () => {
  const original = JSON.stringify(
    {
      tool: 'execute_command',
      arguments: {
        content: 'docker compose ps 2>&1',
        workdir: 'C:\\workspace'
      }
    },
    null,
    2
  );
  const debugText = buildWorkflowToolCallDebugText({
    key: 'cmd-2',
    toolName: 'execute_command',
    toolDisplayName: '执行命令',
    toolRuntimeName: 'execute_command',
    toolFunctionName: 'execute_command',
    callItem: {
      id: 'call-2',
      eventType: 'tool_call',
      toolName: 'execute_command',
      toolCallId: 'call-2',
      commandSessionId: 'cmd-2',
      toolCallRawDetail: original,
      detail: JSON.stringify({
        command: 'docker compose ps 2>&1',
        command_session_id: 'cmd-2',
        exit_code: 0,
        stdout_tail: 'NAME IMAGE COMMAND'
      })
    },
    outputItem: null,
    resultItem: null
  });

  assert.equal(debugText, original);
  assert.ok(debugText.includes('"workdir": "C:\\\\workspace"'));
  assert.ok(!debugText.includes('stdout_tail'));
});

test('command tool call debug text reads saved model call from result-only row', () => {
  const original = JSON.stringify(
    {
      tool: 'execute_command',
      arguments: {
        content: 'rm "/workspace/file.txt"'
      }
    },
    null,
    2
  );
  const debugText = buildWorkflowToolCallDebugText({
    key: 'cmd-3',
    toolName: '执行命令',
    toolDisplayName: '执行命令',
    toolRuntimeName: '执行命令',
    toolFunctionName: 'execute_command',
    callItem: null,
    outputItem: null,
    resultItem: {
      id: 'call-3',
      eventType: 'tool_result',
      toolName: '执行命令',
      toolFunctionName: 'execute_command',
      toolCallId: 'call-3',
      toolCallRawDetail: original,
      toolResultRawDetail: '{"ok":true}',
      detail: JSON.stringify({
        model_observation: '{"ok":true}',
        command_session_id: 'cmd-3'
      })
    }
  });

  assert.equal(debugText, original);
  assert.ok(debugText.includes('rm'));
  assert.ok(!debugText.includes('model_observation'));
});
