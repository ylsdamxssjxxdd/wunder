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

test('message stats context uses roundUsage.total_tokens when explicit context is absent', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        roundUsage: {
          input_tokens: 4027,
          output_tokens: 171,
          total_tokens: 4198
        },
        usage: {
          input_tokens: 3900,
          output_tokens: 180,
          total_tokens: 4080
        }
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Context'), '4198');
});

test('message stats context prefers roundUsage.total_tokens over usage.total_tokens and explicit context_tokens', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        context_tokens: 7101,
        roundUsage: {
          input_tokens: 7180,
          output_tokens: 47,
          total_tokens: 7227
        },
        usage: {
          input_tokens: 7101,
          output_tokens: 126,
          total_tokens: 7300
        }
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Context'), '7227');
});

test('message stats context falls back to usage.total_tokens when roundUsage is absent', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        context_tokens: 2783,
        usage: {
          input_tokens: 9244,
          output_tokens: 12,
          total_tokens: 9268
        }
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Context'), '9268');
});

test('message stats context falls back to usage.input_tokens when total is absent', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        usage: {
          input_tokens: 4027,
          output_tokens: 171,
        }
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Context'), '4027');
});

test('message stats context supports explicit context_occupancy_tokens alias', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        context_occupancy_tokens: 6123
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Context'), '6123');
});

test('message stats clamps direct outlier speed to multi-round average speed', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        usage: {
          input_tokens: 19897,
          output_tokens: 1218,
          total_tokens: 21115
        },
        decode_duration_s: 0.23,
        avg_model_round_speed_tps: 1800,
        avg_model_round_speed_rounds: 4
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Speed'), '1800.00 token/s');
});
