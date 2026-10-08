import test from 'node:test';
import assert from 'node:assert/strict';

import { isAgentAlreadyOpen } from '../../src/views/messenger/agentOpenState';

test('agent open guard ignores selected-only UI state when another session is active', () => {
  assert.equal(
    isAgentAlreadyOpen('agent_target', {
      activeSessionId: 'sess_other',
      activeConversationKey: 'agent:sess_other',
      draftAgentId: '',
      sessions: [{ id: 'sess_other', agent_id: 'agent_other' }]
    }),
    false
  );
});

test('agent open guard recognizes the real active session agent', () => {
  assert.equal(
    isAgentAlreadyOpen('agent_current', {
      activeSessionId: 'sess_current',
      activeConversationKey: 'agent:sess_current',
      draftAgentId: '',
      sessions: [{ id: 'sess_current', agent_id: 'agent_current' }]
    }),
    true
  );
});

test('agent open guard recognizes active draft conversations', () => {
  assert.equal(
    isAgentAlreadyOpen('agent_draft', {
      activeSessionId: '',
      activeConversationKey: 'agent:draft:agent_draft',
      draftAgentId: 'agent_draft',
      sessions: []
    }),
    true
  );
});
