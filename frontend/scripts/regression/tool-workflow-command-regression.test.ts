import test from 'node:test';
import assert from 'node:assert/strict';

import { buildCommandCardView } from '../../src/components/chat/toolWorkflowActionViews';

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
