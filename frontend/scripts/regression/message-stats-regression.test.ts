import test from 'node:test';
import assert from 'node:assert/strict';

import { buildAssistantMessageStatsEntries } from '../../src/utils/messageStats';

const createTranslator = () => {
  const table: Record<string, string> = {
    'chat.stats.duration': 'Duration',
    'chat.stats.speed': 'Speed',
    'chat.stats.contextTokens': 'Context',
    'chat.stats.toolCalls': 'Tools'
  };
  return (key: string) => table[key] || key;
};

const findEntryValue = (
  entries: Array<{ label: string; value: string }>,
  label: string
): string | null => {
  const matched = entries.find((item) => item.label === label);
  return matched ? String(matched.value || '') : null;
};

test('message stats prefer final-round decode speed over aggregated average speed', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        usage: {
          input_tokens: 9731,
          output_tokens: 171,
          total_tokens: 9902
        },
        decode_duration_s: 1.169925369,
        decode_duration_total_s: 1.26708126,
        avg_model_round_speed_tps: 1050.45,
        avg_model_round_speed_rounds: 2
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Speed'), '146.16 token/s');
});

test('message stats suppresses multi-round aggregate speed fallback when usage is absent', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        avg_model_round_speed_tps: 1050.45,
        avg_model_round_speed_rounds: 2,
        context_tokens: 4027
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Speed'), '-');
});
