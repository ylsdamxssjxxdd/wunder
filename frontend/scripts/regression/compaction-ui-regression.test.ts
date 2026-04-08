import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildCompactionDisplay,
  resolveCompactionInstanceLabel
} from '../../src/utils/chatCompactionUi';

const createTranslator = () => {
  const table: Record<string, string> = {
    'chat.toolWorkflow.title': 'Agent Loop',
    'chat.toolWorkflow.compaction.title': 'Context compaction',
    'chat.toolWorkflow.compaction.titleHistory': 'History compaction',
    'chat.toolWorkflow.compaction.titleOverflow': 'Overflow compaction',
    'chat.toolWorkflow.compaction.titleRecovery': 'Recovery compaction',
    'chat.toolWorkflow.compaction.summaryDefault': 'Context was compacted.',
    'chat.toolWorkflow.compaction.summaryHistory': 'Older history was compacted.',
    'chat.toolWorkflow.compaction.summaryOverflow': 'Compaction ran before overflow.',
    'chat.toolWorkflow.compaction.summaryRecovery': 'Compaction recovered this turn.',
    'chat.toolWorkflow.compaction.summaryGuardOnly': 'Only the context guard trimmed content.',
    'chat.toolWorkflow.compaction.summarySkipped': 'Compaction was skipped.',
    'chat.toolWorkflow.compaction.summaryRunningLive': 'Compaction is running.',
    'chat.toolWorkflow.compaction.summaryRecoveringLive': 'Recovering from overflow.',
    'chat.toolWorkflow.compaction.summaryGuardLive': 'Running context guard.',
    'chat.toolWorkflow.compaction.summaryFailedOverflow': 'Compaction failed after overflow.',
    'chat.toolWorkflow.compaction.summaryFallbackAppend': 'Fallback summary was used.',
    'chat.toolWorkflow.compaction.notePrepared': 'Compaction finished.',
    'chat.toolWorkflow.compaction.noteRecovered': 'Recovered after overflow.',
    'chat.toolWorkflow.compaction.noteFallback': 'Fallback summary was used.',
    'chat.toolWorkflow.compaction.noteGuardOnly': 'Only the guard was applied.',
    'chat.toolWorkflow.compaction.noteSkipped': 'Compaction was skipped.',
    'chat.toolWorkflow.compaction.noteRunningLive': 'Compaction is running.',
    'chat.toolWorkflow.compaction.noteRecoveringLive': 'Recovering from overflow.',
    'chat.toolWorkflow.compaction.noteGuardLive': 'Running context guard.',
    'chat.toolWorkflow.compaction.noteFailedOverflow': 'Compaction failed after overflow.',
    'chat.toolWorkflow.compaction.reason.default': 'Compaction',
    'chat.toolWorkflow.compaction.reason.history': 'History',
    'chat.toolWorkflow.compaction.reason.overflow': 'Overflow',
    'chat.toolWorkflow.compaction.reason.overflowRecovery': 'Overflow recovery',
    'chat.toolWorkflow.compaction.detail.reason': 'Reason',
    'chat.toolWorkflow.compaction.detail.projectedRequest': 'Projected request',
    'chat.toolWorkflow.compaction.detail.messageContext': 'Message context',
    'chat.toolWorkflow.compaction.detail.requestBudget': 'Budget',
    'chat.toolWorkflow.compaction.detail.currentQuestion': 'Current question',
    'chat.toolWorkflow.compaction.detail.summary': 'Summary',
    'chat.toolWorkflow.compaction.detail.persistedBaseline': 'Persisted baseline',
    'chat.toolWorkflow.compaction.detail.resetMode': 'Reset mode',
    'chat.toolWorkflow.compaction.detail.errorCode': 'Error code',
    'chat.toolWorkflow.compaction.detail.errorMessage': 'Error message',
    'chat.toolWorkflow.compaction.detail.result': 'Result',
    'chat.toolWorkflow.compaction.detail.resultDone': 'Completed',
    'chat.toolWorkflow.compaction.detail.resultFailed': 'Failed',
    'chat.toolWorkflow.compaction.detail.resultFallback': 'Fallback',
    'chat.toolWorkflow.compaction.detail.resultGuardOnly': 'Guard only',
    'chat.toolWorkflow.compaction.detail.resultSkipped': 'Skipped',
    'chat.toolWorkflow.compaction.detail.valuePreserved': 'Preserved',
    'chat.toolWorkflow.compaction.detail.valueTrimmed': 'Trimmed',
    'chat.toolWorkflow.compaction.detail.valueRemoved': 'Removed',
    'chat.toolWorkflow.compaction.output.modelTitle': 'Compaction model output',
    'chat.toolWorkflow.compaction.output.fallbackTitle': 'Fallback compaction output',
    'chat.toolWorkflow.compaction.output.injectedTitle': 'Injected context summary',
    'chat.toolWorkflow.compaction.output.pending': 'Waiting for compaction output.',
    'chat.toolWorkflow.compaction.output.empty': 'No compaction output.',
    'chat.toolWorkflow.compaction.output.emptyGuardOnly': 'Guard only, no new compaction output.',
    'chat.toolWorkflow.compaction.output.emptySkipped': 'Skipped, no new compaction output.',
    'chat.toolWorkflow.compaction.usage.before': 'Before {tokens} ({percent})',
    'chat.toolWorkflow.compaction.usage.after': 'After {tokens} ({percent})',
    'chat.toolWorkflow.compaction.stage.detect': 'Detect',
    'chat.toolWorkflow.compaction.stage.compact': 'Compact',
    'chat.toolWorkflow.compaction.stage.guard': 'Guard',
    'chat.toolWorkflow.compaction.stage.resume': 'Resume',
    'chat.toolWorkflow.compaction.stage.pending': 'Pending',
    'chat.toolWorkflow.compaction.stage.notNeeded': 'Not needed'
  };
  return (key: string, params?: Record<string, unknown>) => {
    const template = table[key] || key;
    if (!params) return template;
    return template.replace(/\{(\w+)\}/g, (_match, token) => String(params[token] ?? ''));
  };
};

test('compaction display shows actual model output and injected summary', () => {
  const display = buildCompactionDisplay(
    {
      status: 'done',
      reason: 'history',
      summary_model_output: 'Model compacted the last three turns into one summary.',
      summary_text: 'Injected summary with memory block.',
      projected_request_tokens: 9210,
      projected_request_tokens_after: 4120,
      max_context: 16384,
      limit: 16384
    },
    'completed',
    createTranslator()
  );

  assert.equal(display.view.outputs.length, 2);
  assert.equal(display.view.outputs[0]?.title, 'Compaction model output');
  assert.match(display.view.outputs[0]?.body || '', /last three turns/);
  assert.equal(display.view.outputs[1]?.title, 'Injected context summary');
  assert.match(display.copyBody, /Injected context summary/);
  assert.equal(display.view.usageBar?.beforeRatio, 9210 / 16384);
  assert.equal(display.view.usageBar?.beforeBarRatio, 9210 / 16384);
});

test('compaction display does not fabricate output for guard-only runs', () => {
  const display = buildCompactionDisplay(
    {
      status: 'guard_only',
      reason: 'overflow',
      context_guard_applied: true
    },
    'completed',
    createTranslator()
  );

  assert.equal(display.view.outputs.length, 0);
  assert.equal(display.view.outputEmpty, 'Guard only, no new compaction output.');
});

test('compaction usage percent keeps real overflow ratio while bar width stays clamped', () => {
  const display = buildCompactionDisplay(
    {
      status: 'done',
      reason: 'history',
      projected_request_tokens: 37356,
      projected_request_tokens_after: 18200,
      max_context: 40192,
      limit: 36172
    },
    'completed',
    createTranslator()
  );

  assert.equal(display.view.usageBar?.beforeRatio, 37356 / 40192);
  assert.equal(display.view.usageBar?.beforeBarRatio, 37356 / 40192);
  assert.match(display.view.usageBar?.beforeLabel || '', /93%/);
  assert.equal(display.view.usageBar?.afterBarRatio, 18200 / 40192);
});

test('compaction usage falls back to compaction limit when model max context is missing', () => {
  const display = buildCompactionDisplay(
    {
      status: 'done',
      reason: 'history',
      projected_request_tokens: 44620,
      projected_request_tokens_after: 18200,
      limit: 40192
    },
    'completed',
    createTranslator()
  );

  assert.equal(display.view.usageBar?.beforeRatio, 44620 / 40192);
  assert.equal(display.view.usageBar?.beforeBarRatio, 1);
  assert.match(display.view.usageBar?.beforeLabel || '', /111%/);
});

test('compaction instance label distinguishes auto and manual runs', () => {
  const t = createTranslator();

  assert.equal(resolveCompactionInstanceLabel('compaction:4:2', t), 'Compaction #2');
  assert.equal(resolveCompactionInstanceLabel('compaction:auto:3', t), 'Compaction #3');
  assert.equal(resolveCompactionInstanceLabel('compaction:manual:123456', t), 'Manual compaction');
  assert.equal(resolveCompactionInstanceLabel('tool:other:1', t), '');
});
