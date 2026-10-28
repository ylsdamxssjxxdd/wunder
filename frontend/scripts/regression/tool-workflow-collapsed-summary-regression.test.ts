import test from 'node:test';
import assert from 'node:assert/strict';

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
    contextTokensLabel: '4.1k token',
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
    contextTokensLabel: '8.2k token',
    contextTokensSource: 'result',
    consumedTokensLabel: '654 token',
    consumedTokensSource: 'result',
    durationLabel: '612ms'
  });
});

test('skill rows summarize the loaded skill name instead of the shared entry resource', () => {
  // 技能调用对所有技能共用一个工具；摘要取 path 会一律得到 SKILL.md。
  for (const name of ['skill_call', 'skill_get', '\u6280\u80fd\u8c03\u7528']) {
    const summary = buildCollapsedToolWorkflowSummary({
      key: `skill:${name}`,
      toolName: name,
      toolDisplayName: '',
      toolRuntimeName: name,
      toolFunctionName: '',
      callItem: {
        toolCallRawDetail: JSON.stringify({
          name: '\u516c\u6587\u5199\u4f5c',
          action: 'read',
          path: 'SKILL.md'
        })
      },
      outputItem: null,
      resultItem: {
        detail: JSON.stringify({ data: { name: '\u516c\u6587\u5199\u4f5c', path: 'SKILL.md' } })
      }
    }, '\u6280\u80fd');

    assert.equal(summary.brief, '\u516c\u6587\u5199\u4f5c');
    assert.equal(summary.title, '\u6280\u80fd \u516c\u6587\u5199\u4f5c');
    assert.doesNotMatch(summary.title, /SKILL\.md/);
  }
});

test('skill rows fall back to the persisted result name and never to SKILL.md', () => {
  const summary = buildCollapsedToolWorkflowSummary({
    key: 'skill:result-only',
    toolName: 'skill_call',
    toolDisplayName: '',
    toolRuntimeName: 'skill_call',
    toolFunctionName: '',
    callItem: {
      detail: JSON.stringify({ data: { name: 'demo_skill', path: 'SKILL.md' } })
    },
    outputItem: null,
    resultItem: {
      toolCallRawDetail: JSON.stringify({ skill_name: 'demo_skill', path: 'SKILL.md' })
    }
  }, '\u6280\u80fd');

  assert.equal(summary.brief, 'demo_skill');
  assert.equal(summary.title, '\u6280\u80fd demo_skill');
});
