import test from 'node:test';
import assert from 'node:assert/strict';
import { createChatRuntimeProjection, applyChatRuntimeEvent } from '../../src/realtime/chat/chatRuntimeReducer';
import { buildCanonicalChatRuntimeEvents } from '../../src/realtime/chat/chatCanonicalEvents';
import { readQueueSchedulingState } from '../../src/utils/queueScheduling';
import { buildCanonicalSessionEventsSnapshot } from '../../src/realtime/chat/chatRuntimeBridge';

test('queue handoff keeps existing output and resumes the same model turn', () => {
  const projection = createChatRuntimeProjection();
  const ids = { session_id:'session-a', user_turn_id:'turn-a', model_turn_id:'model-a', message_id:'message-a', strict:false };
  applyChatRuntimeEvent(projection, { ...ids, event_type:'assistant_delta', delta:'retained' });
  applyChatRuntimeEvent(projection, { ...ids, event_type:'queue_status', payload:{source_event_type:'queue_enter', reason:'admin_preempted', queue_state:'suspended'} });
  assert.equal(projection.sessions['session-a'].runtimeStatus, 'queued');
  applyChatRuntimeEvent(projection, { ...ids, event_type:'queue_status', payload:{source_event_type:'queue_start', resumed:true} });
  const session = projection.sessions['session-a'];
  assert.deepEqual([session.runtimeStatus, session.messageById['message-a'].content, session.messageById['message-a'].status], ['running', 'retained', 'streaming']);
  assert.equal(Object.keys(session.modelTurnById).length, 1);
});

test('queue cancellation stays cancelled rather than failed', () => {
  const events = buildCanonicalChatRuntimeEvents({sessionId:'session-a', eventType:'queue_fail', payload:{status:'cancelled'}, eventId:'1'});
  assert.equal(events[0].event_type, 'turn_cancelled');
});

test('queue scheduling detail tolerates malformed history', () => {
  assert.deepEqual([
    readQueueSchedulingState('{'),
    readQueueSchedulingState({reason:'admin_preempted', queue_state:'pausing'}),
    readQueueSchedulingState(JSON.stringify({reason:'admin_preempted', queue_state:'suspended'})),
    readQueueSchedulingState({queue_priority:1})
  ], ['', 'pausing', 'suspended', 'priority']);
});

test('runtime snapshot preserves suspended queue state after refresh', () => {
  const events = buildCanonicalSessionEventsSnapshot({
    sessionId: 'session-a',
    phase: 'snapshot',
    payload: {
      runtime: {
        thread_status: 'queued',
        queue_state: 'suspended',
        queue_reason: 'admin_preempted'
      }
    }
  });
  assert.equal(events[0]?.event_type, 'session_runtime');
  assert.deepEqual(
    [events[0]?.payload.queue_state, events[0]?.payload.queue_reason],
    ['suspended', 'admin_preempted']
  );
});
