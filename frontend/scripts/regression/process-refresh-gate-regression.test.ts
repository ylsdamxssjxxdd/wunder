import assert from 'node:assert/strict';
import { completeProcessRefresh, requestProcessRefresh, type ProcessRefreshGateState } from '../../src/components/chat/processRefreshGate';

const state: ProcessRefreshGateState = { pending: false };

assert.equal(requestProcessRefresh(state, true), true);
assert.equal(state.pending, false);

assert.equal(requestProcessRefresh(state, false), false);
assert.equal(state.pending, true);
assert.equal(completeProcessRefresh(state, false), false);
assert.equal(state.pending, true);
assert.equal(completeProcessRefresh(state, true), true);
assert.equal(state.pending, false);

assert.equal(completeProcessRefresh(state, true), false);
assert.equal(state.pending, false);
console.log('process refresh gate: in-flight refresh is retained and replayed once after completion');
