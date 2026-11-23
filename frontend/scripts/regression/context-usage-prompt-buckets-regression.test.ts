import test from 'node:test';
import assert from 'node:assert/strict';

import {
  applyChatLiveContextUsageEvent,
  readChatLiveContextUsage,
  resetChatLiveContextUsage
} from '../../src/realtime/chat/chatLiveContextUsage';
import { resolvePromptTokens } from '../../src/utils/tokenUsage';

test('prompt buckets are summed back into one prompt size', () => {
  assert.equal(resolvePromptTokens({ input_tokens: 300, cache_read_tokens: 190000 }), 190300);
  assert.equal(
    resolvePromptTokens({ input_tokens: 10, cache_read_input_tokens: 80, cache_creation_input_tokens: 10 }),
    100
  );
});

test('provider-native inclusive prompt_tokens never double count cache', () => {
  assert.equal(resolvePromptTokens({ prompt_tokens: 1000, cached_tokens: 600 }), 1000);
  assert.equal(
    resolvePromptTokens({
      input_tokens: 100,
      input_tokens_details: { cached_tokens: 80, cache_write_tokens: 5 }
    }),
    100
  );
  assert.equal(resolvePromptTokens({ input_tokens: 12 }), 12);
  assert.equal(resolvePromptTokens(null), null);
});

test('composer gauge reads the whole prompt from a usage frame, not the uncached share', () => {
  resetChatLiveContextUsage();
  applyChatLiveContextUsageEvent('s1', 'llm_output', {
    usage: { input_tokens: 312, cache_read_tokens: 190_000, output_tokens: 88, total_tokens: 190_400 }
  });
  assert.equal(readChatLiveContextUsage('s1')?.used, 190_312);
});

test('explicit occupancy still wins over a usage frame', () => {
  resetChatLiveContextUsage();
  applyChatLiveContextUsageEvent('s2', 'llm_output', { usage: { input_tokens: 5, cache_read_tokens: 100 } });
  applyChatLiveContextUsageEvent('s2', 'context_usage', {
    context_occupancy_tokens: 191_000,
    context_usage_source: 'provider_input'
  });
  assert.equal(readChatLiveContextUsage('s2')?.used, 191_000);
});
