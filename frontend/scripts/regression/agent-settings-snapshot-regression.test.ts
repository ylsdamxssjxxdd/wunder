import test from 'node:test';
import assert from 'node:assert/strict';

import {
  canonicalizeAgentToolName,
  normalizeAgentToolNamesForSettings,
  normalizeAgentToolNamesForSettingsSnapshot,
  resolveDesktopToolKind,
  resolveWebToolKind
} from '../../src/utils/agentSettingsSnapshot';
import { buildDeclaredDependencyPayload } from '../../src/utils/agentDependencyStatus';

const WEB_FETCH_TOOL_NAME = '\u7f51\u9875\u6293\u53d6';
const WEB_SEARCH_TOOL_NAME = '\u7f51\u9875\u641c\u7d22';

test('agent settings normalize desktop tool aliases to persisted runtime names', () => {
  assert.equal(resolveDesktopToolKind('desktop_controller'), 'controller');
  assert.equal(resolveDesktopToolKind('desktop monitor'), 'monitor');
  assert.equal(canonicalizeAgentToolName('desktop_controller'), '桌面控制器');
  assert.equal(canonicalizeAgentToolName('desktop_monitor'), '桌面监视器');
  assert.deepEqual(
    normalizeAgentToolNamesForSettings([
      'desktop_controller',
      '桌面控制器',
      'desktop_monitor',
      '桌面监视器'
    ]),
    ['桌面控制器', '桌面监视器']
  );
});

test('agent settings normalize web tool aliases to persisted runtime names', () => {
  assert.equal(resolveWebToolKind('web_fetch'), 'fetch');
  assert.equal(resolveWebToolKind('web fetch'), 'fetch');
  assert.equal(resolveWebToolKind('web-fetch'), 'fetch');
  assert.equal(resolveWebToolKind(WEB_FETCH_TOOL_NAME), 'fetch');
  assert.equal(resolveWebToolKind('web_search'), 'search');
  assert.equal(resolveWebToolKind('web search'), 'search');
  assert.equal(resolveWebToolKind(WEB_SEARCH_TOOL_NAME), 'search');
  assert.equal(canonicalizeAgentToolName('web_fetch'), WEB_FETCH_TOOL_NAME);
  assert.equal(canonicalizeAgentToolName('web_search'), WEB_SEARCH_TOOL_NAME);
  assert.deepEqual(
    normalizeAgentToolNamesForSettings([
      'web_fetch',
      WEB_FETCH_TOOL_NAME,
      'web-search',
      WEB_SEARCH_TOOL_NAME
    ]),
    [WEB_FETCH_TOOL_NAME, WEB_SEARCH_TOOL_NAME]
  );
});

test('agent settings snapshots compare canonical saved dependency payloads', () => {
  const catalog = {
    builtin_tools: [
      { name: '桌面控制器' },
      { name: '桌面监视器' }
    ],
    skills: [
      { name: 'summarize' }
    ]
  };
  const currentAgent = {
    tool_names: ['desktop_controller', 'summarize'],
    declared_tool_names: ['stale_missing_tool'],
    declared_skill_names: ['summarize']
  };
  const selectedToolNames = normalizeAgentToolNamesForSettings([
    'desktop_controller',
    'summarize'
  ]);
  const payload = buildDeclaredDependencyPayload(selectedToolNames, currentAgent, catalog);

  assert.deepEqual(payload.tool_names, ['桌面控制器', 'summarize']);
  assert.deepEqual(payload.declared_tool_names, ['桌面控制器', 'stale_missing_tool']);
  assert.deepEqual(payload.declared_skill_names, ['summarize']);
  assert.deepEqual(normalizeAgentToolNamesForSettingsSnapshot(payload.tool_names), [
    'summarize',
    '桌面控制器'
  ]);
});
