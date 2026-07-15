import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildCollapsedToolWorkflowSummary,
  isReadImageWorkflowTool
} from '../../src/components/chat/toolWorkflowCollapsedSummary';

test('collapsed workflow rows retain a bounded command summary without parsing full result payloads', () => {
  const summary = buildCollapsedToolWorkflowSummary({
    key: 'command-1',
    toolName: 'execute_command',
    toolDisplayName: '',
    toolRuntimeName: 'execute_command',
    toolFunctionName: '',
    callItem: {
      detail: JSON.stringify({
        args: { content: 'node scripts/check.mjs --quick' }
      })
    },
    outputItem: null,
    resultItem: {
      detail: JSON.stringify({ data: { output: 'x'.repeat(200_000) } })
    }
  }, '执行');

  assert.equal(summary.brief, 'node scripts/check.mjs --quick');
  assert.equal(summary.title, '执行 node scripts/check.mjs --quick');
});

test('read image aliases render as one visible image-reading tool with a path summary', () => {
  for (const name of ['read_image', 'view_image', '\u8bfb\u56fe\u5de5\u5177']) {
    assert.equal(isReadImageWorkflowTool(name), true);
    const summary = buildCollapsedToolWorkflowSummary({
      key: `image:${name}`,
      toolName: name,
      toolDisplayName: '',
      toolRuntimeName: name,
      toolFunctionName: '',
      callItem: {
        detail: JSON.stringify({ args: { path: '/workspace/assets/preview.png' } })
      },
      outputItem: null,
      resultItem: null
    }, '\u8bfb\u56fe');
    assert.equal(summary.brief, '.../assets/preview.png');
    assert.equal(summary.title, '\u8bfb\u56fe .../assets/preview.png');
  }
});
