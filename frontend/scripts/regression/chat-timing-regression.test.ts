import test from 'node:test';
import assert from 'node:assert/strict';

import { normalizeChatDurationSeconds, normalizeChatTimestampMs } from '../../src/utils/chatTiming';

test('normalizes chat duration values that accidentally arrive in milliseconds', () => {
  assert.equal(normalizeChatDurationSeconds(32_000), 32);
});

test('normalizes chat duration values that accidentally arrive in microseconds', () => {
  assert.equal(normalizeChatDurationSeconds(32_000_000), 32);
});

test('normalizes chat duration values that accidentally arrive in nanoseconds', () => {
  assert.equal(normalizeChatDurationSeconds(32_000_000_000), 32);
});

test('keeps already-correct second durations untouched', () => {
  assert.equal(normalizeChatDurationSeconds(12.5), 12.5);
});

test('accepts epoch timestamps expressed in seconds', () => {
  assert.equal(normalizeChatTimestampMs(1_710_000_000), 1_710_000_000_000);
});

test('accepts epoch timestamps expressed in microseconds', () => {
  assert.equal(normalizeChatTimestampMs(1_710_000_000_000_000), 1_710_000_000_000);
});

test('rejects implausible numeric timestamps that are not wall clock times', () => {
  assert.equal(normalizeChatTimestampMs(32_000_000), null);
});
