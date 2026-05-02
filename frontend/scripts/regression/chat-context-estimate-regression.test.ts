import test from 'node:test';
import assert from 'node:assert/strict';

import { estimateRequestContextTokens } from '../../src/utils/chatContextEstimate';

const buildHeavyToolSpecs = (count = 46) =>
  Array.from({ length: count }, (_, index) => ({
    type: 'function',
    function: {
      name: `tool_${index}`,
      description: 'Use this tool when the task needs structured external work. '.repeat(18),
      parameters: {
        type: 'object',
        properties: {
          input: {
            type: 'string',
            description: 'Detailed input for this operation. '.repeat(10)
          }
        },
        required: ['input']
      }
    }
  }));

test('request context estimate includes tool specs instead of only messages', () => {
  const messages = [
    { role: 'system', content: 'short system prompt' },
    { role: 'user', content: 'hello' }
  ];
  const tools = buildHeavyToolSpecs();

  const estimate = estimateRequestContextTokens({
    request: {
      payload: {
        model: 'test-model',
        messages,
        tools,
        tool_choice: 'auto'
      }
    }
  });

  assert.ok(estimate !== null);
  assert.ok(estimate > 12000, `estimate should include large tool specs, got ${estimate}`);
});

test('request context estimate stays near real debug scale for system prompt and many tools', () => {
  const messages = [
    {
      role: 'system',
      content: [
        'You are an agent that must use available tools when needed.',
        'Follow environment, safety, workflow, and formatting rules carefully.'
      ].join('\n').repeat(80)
    },
    { role: 'user', content: 'hello' }
  ];
  const tools = buildHeavyToolSpecs();

  const estimate = estimateRequestContextTokens({
    request: {
      payload: {
        model: 'test-model',
        messages,
        tools,
        tool_choice: 'auto',
        chat_template_kwargs: { enable_thinking: false },
        stream_options: { include_usage: true }
      }
    }
  });

  assert.ok(estimate !== null);
  assert.ok(estimate > 20000, `estimate should not regress to a few thousand tokens, got ${estimate}`);
});

test('request context estimate still works for message-only payloads', () => {
  const estimate = estimateRequestContextTokens({
    payload: {
      messages: [
        { role: 'system', content: 'You are concise.' },
        { role: 'user', content: 'Summarize the file.' }
      ]
    }
  });

  assert.ok(estimate !== null);
  assert.ok(estimate > 0);
  assert.ok(estimate < 500);
});

test('request context estimate handles summary-only payloads conservatively', () => {
  const estimate = estimateRequestContextTokens({
    payload_omitted: true,
    payload_summary: {
      messages: { count: 2, role_counts: { system: 1, user: 1 } },
      tools: { count: 46, preview: [], truncated: true }
    }
  });

  assert.ok(estimate !== null);
  assert.ok(estimate > 18000, `summary-only estimate should not collapse to message-only size, got ${estimate}`);
});

test('request context estimate finds nested summary-only payloads', () => {
  const estimate = estimateRequestContextTokens({
    data: {
      request: {
        payload_omitted: true,
        payload_summary: {
          messages: { count: 2 },
          tools: { count: 46, truncated: true }
        }
      }
    }
  });

  assert.ok(estimate !== null);
  assert.ok(estimate > 18000, `nested summary-only estimate should include tool scale, got ${estimate}`);
});
