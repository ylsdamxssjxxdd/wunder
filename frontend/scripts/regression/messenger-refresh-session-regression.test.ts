import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

test('conversation refresh retains the selected session without a hard reload fallback', () => {
  const source = readFileSync(
    resolve(process.cwd(), 'src/views/messenger/controller/messengerControllerConversationOpenActions.ts'),
    'utf8'
  );
  const refreshStart = source.indexOf('ctx.handleChatPageRefresh = () => {');
  const refreshEnd = source.indexOf('ctx.handleRightDockSkillArchiveUpload', refreshStart);
  assert.ok(refreshStart >= 0);
  assert.ok(refreshEnd > refreshStart);
  const refreshSource = source.slice(refreshStart, refreshEnd);
  assert.ok(refreshSource.includes('ctx.refreshActiveAgentConversation()'));
  assert.ok(!refreshSource.includes('window.location.reload'));

  const openSessionStart = source.indexOf('ctx.openAgentSession = async');
  assert.ok(openSessionStart >= 0);
  const openSessionSource = source.slice(openSessionStart);
  assert.ok(openSessionSource.includes('forceHydrateForeground: true'));
  assert.ok(!openSessionSource.includes('await ctx.openAgentById(fallbackAgentId || DEFAULT_AGENT_KEY);'));
});
