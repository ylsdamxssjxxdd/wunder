import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import {
  buildCollapsedToolWorkflowSummary,
  isReadImageWorkflowTool
} from '../../src/components/chat/toolWorkflowCollapsedSummary';
import { resolveCollapsedWorkflowEntryMetadata } from '../../src/components/chat/toolWorkflowCollapsedMetadata';

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
  }, '\u6267\u884c');

  assert.equal(summary.brief, 'node scripts/check.mjs --quick');
  assert.equal(summary.title, '\u6267\u884c node scripts/check.mjs --quick');
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
    assert.equal(summary.brief, 'preview.png');
    assert.equal(summary.title, '\u8bfb\u56fe preview.png');
  }
});

test('workflow component keeps the same bounded summary after a row expands', () => {
  const source = readFileSync(
    resolve(process.cwd(), 'src/components/chat/MessageToolWorkflow.vue'),
    'utf8'
  );
  assert.ok(source.includes('const collapsedSummary = buildCollapsedToolWorkflowSummary(entry, toolDisplay);'));
  assert.ok(source.includes('summaryBrief: collapsedSummary.brief || summary.summaryBrief'));
  assert.ok(source.includes('grid-template-columns: 144px minmax(0, 1fr)'));
});

test('collapsed workflow rows retain token and duration metadata without parsing a full result', () => {
  const metadata = resolveCollapsedWorkflowEntryMetadata({
    key: 'metadata-1',
    toolName: 'read_image',
    toolDisplayName: '',
    toolRuntimeName: 'read_image',
    toolFunctionName: '',
    callItem: {
      detail: JSON.stringify({
        payload: {
          request_consumed_tokens: 321,
          context_occupancy_tokens: 4096,
          duration_ms: 1675
        },
        output: 'x'.repeat(200_000)
      })
    },
    outputItem: null,
    resultItem: null
  });

  assert.deepEqual(metadata, {
    contextTokensLabel: '4096 token',
    contextTokensSource: 'call',
    consumedTokensLabel: '321 token',
    consumedTokensSource: 'call',
    durationLabel: '1.7s'
  });
});

test('collapsed workflow metadata reads terminal fields from a bounded result tail', () => {
  const metadata = resolveCollapsedWorkflowEntryMetadata({
    key: 'metadata-2',
    toolName: 'read_image',
    toolDisplayName: '',
    toolRuntimeName: 'read_image',
    toolFunctionName: '',
    callItem: null,
    outputItem: null,
    resultItem: {
      detail: JSON.stringify({
        output: 'x'.repeat(20_000),
        result: {
          meta: {
            request_consumed_tokens: 654,
            context_occupancy_tokens: 8192,
            elapsed_ms: 612
          }
        }
      })
    }
  });

  assert.deepEqual(metadata, {
    contextTokensLabel: '8192 token',
    contextTokensSource: 'result',
    consumedTokensLabel: '654 token',
    consumedTokensSource: 'result',
    durationLabel: '612ms'
  });
});
