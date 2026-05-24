import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

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

test('conversation open actions use the shared agent open guard', () => {
  const source = readFileSync(
    resolve(process.cwd(), 'src/views/messenger/controller/messengerControllerConversationOpenActions.ts'),
    'utf8'
  );
  assert.ok(source.includes('isAgentAlreadyOpen'));
});

test('desktop companion open command can use the active messenger open handler before route fallback', () => {
  const floatingLayerSource = readFileSync(
    resolve(process.cwd(), 'src/components/companions/CompanionFloatingLayer.vue'),
    'utf8'
  );
  const lifecycleSource = readFileSync(
    resolve(process.cwd(), 'src/views/messenger/controller/messengerControllerLifecycleReactiveEffects.ts'),
    'utf8'
  );
  assert.ok(floatingLayerSource.includes('openCompanionAgent'));
  assert.ok(lifecycleSource.includes('registerCompanionOpenHandler'));
  assert.ok(floatingLayerSource.indexOf('openCompanionAgent') < floatingLayerSource.indexOf('router.replace'));
});

test('openAgentById still restores the chat shell when the target agent is already active', () => {
  const source = readFileSync(
    resolve(process.cwd(), 'src/views/messenger/controller/messengerControllerConversationOpenActions.ts'),
    'utf8'
  );
  assert.ok(source.includes('await ctx.openAgentSession(activeSessionId, normalized);'));
  assert.ok(source.includes('await ctx.openAgentDraftSessionWithScroll(normalized);'));
});

test('route restore treats agent_id outside messages as settings selection only', () => {
  const source = readFileSync(
    resolve(process.cwd(), 'src/views/messenger/controller/messengerControllerLifecycleRouteBootstrap.ts'),
    'utf8'
  );
  const nonMessageGuardIndex = source.indexOf("if (querySection !== 'messages')");
  const openAgentIndex = source.indexOf('await ctx.openAgentById(targetAgentId);');
  const watcherGuardIndex = source.indexOf("resolveSectionFromRoute(ctx.route.path, ctx.route.query.section) !== 'messages'");
  assert.ok(nonMessageGuardIndex >= 0);
  assert.ok(openAgentIndex > nonMessageGuardIndex);
  assert.ok(watcherGuardIndex >= 0);
  assert.ok(source.includes("querySection === 'agents' && queryAgentId"));
  assert.ok(source.includes("ctx.selectedAgentId.value = ctx.normalizeAgentId(queryAgentId);"));
});

test('route watcher never restores agent conversations from non-message sections', () => {
  const source = readFileSync(
    resolve(process.cwd(), 'src/views/messenger/controller/messengerControllerLifecycleRouteBootstrap.ts'),
    'utf8'
  );
  const watcherStart = source.lastIndexOf('ctx.route.query.session_id');
  const watcherGuard = source.indexOf("resolveSectionFromRoute(ctx.route.path, ctx.route.query.section) !== 'messages'", watcherStart);
  const watcherRestore = source.indexOf('void ctx.restoreConversationFromRoute();', watcherStart);
  assert.ok(watcherStart >= 0);
  assert.ok(watcherGuard > watcherStart);
  assert.ok(watcherRestore > watcherGuard);
});
