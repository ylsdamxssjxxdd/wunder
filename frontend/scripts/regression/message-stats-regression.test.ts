import test from 'node:test';
import assert from 'node:assert/strict';

import { buildAssistantMessageStatsEntries } from '../../src/utils/messageStats';
import { summarizeTurnDecodeSpeed } from '../../src/utils/turnDecodeSpeed';

const createTranslator = () => {
  const table: Record<string, string> = {
    'chat.stats.duration': 'Duration',
    'chat.stats.speed': 'Speed',
    'chat.stats.contextTokens': 'Context',
    'chat.stats.quota': 'Quota',
    'chat.stats.toolCalls': 'Tools',
    'messenger.messageStatus.compacting': 'Compacting',
    'messenger.messageStatus.requesting': 'Requesting',
    'messenger.messageStatus.waitingInput': 'Waiting input',
    'messenger.messageStatus.done': 'Done',
    'messenger.messageStatus.error': 'Error',
    'messenger.messageStatus.modelOutputting': 'Model outputting',
    'messenger.messageStatus.running': 'Running',
    'messenger.messageStatus.toolRunning': 'Tool running',
    'messenger.messageStatus.queued': 'Queued',
    'messenger.messageStatus.resumable': 'Resumable',
    'messenger.messageStatus.retrying': 'Retrying'
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

test('message stats use backend-provided user-round average decode speed', () => {
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
  assert.equal(findEntryValue(entries, 'Speed'), '1050.45 token/s');
});

test('message stats shows backend average decode speed even when usage is absent', () => {
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
  assert.equal(findEntryValue(entries, 'Speed'), '1050.45 token/s');
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

test('message stats shows quota consumed tokens for the user round', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        quotaConsumed: 4198,
        roundUsage: {
          input_tokens: 4027,
          output_tokens: 171,
          total_tokens: 4198
        }
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Quota'), '4198');
});

test('message stats still shows quota consumed after an interrupted response', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stream_incomplete: false,
      stop_reason: 'interrupted',
      stats: {
        quotaConsumed: 1536
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Quota'), '1536');
});

test('message stats falls back to user-round total tokens when quota event is missing', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        quotaConsumed: 1,
        roundUsage: {
          input_tokens: 3870,
          output_tokens: 328,
          total_tokens: 4198
        }
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Quota'), '4198');
});

test('message stats keeps backend average speed without frontend clamping', () => {
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

test('message stats prefers average speed for tool turns', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        toolCalls: 3,
        usage: {
          input_tokens: 19897,
          output_tokens: 1218,
          total_tokens: 21115
        },
        decode_duration_s: 0.23,
        avg_model_round_speed_tps: 312.5,
        avg_model_round_speed_rounds: 4
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Speed'), '312.50 token/s');
});

test('message stats hides tool-turn speed when no reliable average exists', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stats: {
        toolCalls: 1,
        usage: {
          input_tokens: 9244,
          output_tokens: 700,
          total_tokens: 9944
        },
        decode_duration_s: 0.21
      }
    },
    t
  );
  assert.equal(findEntryValue(entries, 'Speed'), '-');
});

test('message stats prefers compacting when another assistant message is running compaction', () => {
  const t = createTranslator();
  const pendingAssistant = {
    role: 'assistant',
    workflowStreaming: true,
    workflowItems: [
      {
        eventType: 'llm_request',
        status: 'loading'
      }
    ]
  };
  const compactionAssistant = {
    role: 'assistant',
    workflowStreaming: true,
    workflowItems: [
      {
        eventType: 'compaction_progress',
        status: 'loading'
      }
    ]
  };

  const entries = buildAssistantMessageStatsEntries(
    pendingAssistant,
    t,
    [pendingAssistant, compactionAssistant]
  );

  assert.equal(entries[0]?.value, 'Compacting');
});

test('message stats keeps waiting-input status ahead of stale running flags', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      stream_incomplete: true,
      questionPanel: {
        status: 'pending'
      },
      stats: {
        usage: {
          total_tokens: 1200
        }
      }
    },
    t
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.value, 'Waiting input');
});

test('message stats suppresses completed metrics while compaction is still running', () => {
  const t = createTranslator();
  const entries = buildAssistantMessageStatsEntries(
    {
      role: 'assistant',
      workflowItems: [
        {
          eventType: 'compaction_progress',
          status: 'loading'
        }
      ],
      stats: {
        usage: {
          total_tokens: 2048
        },
        decode_duration_s: 1.2
      }
    },
    t
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0]?.value, 'Compacting');
});

test('turn decode speed summary matches backend user-round average semantics', () => {
  const summary = summarizeTurnDecodeSpeed([
    {
      prefill: 0.42,
      decode: 2.4,
      usage: {
        output: 120
      }
    },
    {
      prefill: 0.33,
      decode: 1.6,
      usage: {
        output: 80
      }
    },
    {
      prefill: 0.28,
      decode: 0.9,
      usage: null
    }
  ]);

  assert.ok(Math.abs(Number(summary.prefillDurationTotalS) - 1.03) < 1e-9);
  assert.equal(summary.decodeDurationTotalS, 4);
  assert.equal(summary.avgModelRoundSpeedRounds, 2);
  assert.equal(summary.avgModelRoundSpeedTps, 50);
});

test('turn decode speed summary ignores rounds without both decode time and output tokens', () => {
  const summary = summarizeTurnDecodeSpeed([
    {
      decode: 1.2,
      usage: {
        output: 60
      }
    },
    {
      decode: 0,
      usage: {
        output: 100
      }
    },
    {
      decode: 1.1,
      usage: {
        output: 0
      }
    }
  ]);

  assert.equal(summary.decodeDurationTotalS, 1.2);
  assert.equal(summary.avgModelRoundSpeedRounds, 1);
  assert.equal(summary.avgModelRoundSpeedTps, 50);
});
