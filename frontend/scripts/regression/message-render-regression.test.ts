import test from 'node:test';
import assert from 'node:assert/strict';

import { prepareMessageMarkdownContent } from '../../src/utils/messageMarkdown';
import {
  buildAgentWorkspaceScopeId,
  buildWorkspacePublicPathFromScope,
  resolveMarkdownWorkspacePath
} from '../../src/utils/messageWorkspacePath';

test('repairs malformed markdown image closings', () => {
  const content =
    '![冲突地理态势图](https://example.com/map.png?Expires=1773114652&Signature=abc123\\\\\\)';
  const repaired = prepareMessageMarkdownContent(content, null);
  assert.equal(
    repaired,
    '![冲突地理态势图](https://example.com/map.png?Expires=1773114652&Signature=abc123)'
  );
});

test('keeps malformed markdown closings inside fenced code blocks untouched', () => {
  const content = [
    '```md',
    '![冲突地理态势图](https://example.com/map.png?Expires=1773114652&Signature=abc123\\\\\\)',
    '```'
  ].join('\n');
  const repaired = prepareMessageMarkdownContent(content, null);
  assert.equal(repaired, content);
});

test('resolves bare relative paths for container scoped workspaces', () => {
  const resolved = resolveMarkdownWorkspacePath({
    rawPath: 'temp_dir/美以伊冲突完整报告_汇总.md',
    ownerId: 'demo-user',
    containerId: 7
  });
  assert.equal(
    resolved,
    '/workspaces/demo-user__c__7/temp_dir/%E7%BE%8E%E4%BB%A5%E4%BC%8A%E5%86%B2%E7%AA%81%E5%AE%8C%E6%95%B4%E6%8A%A5%E5%91%8A_%E6%B1%87%E6%80%BB.md'
  );
});

test('resolves dot relative paths for normal user world workspaces', () => {
  const resolved = resolveMarkdownWorkspacePath({
    rawPath: './temp_dir/report.md',
    ownerId: 'world-user'
  });
  assert.equal(resolved, '/workspaces/world-user/temp_dir/report.md');
});

test('resolves bare relative paths for agent scoped workspaces', () => {
  const scopeId = buildAgentWorkspaceScopeId('demo-user', 'analysis-agent');
  assert.equal(scopeId, 'demo-user__a__analysis-agent');
  const resolved = resolveMarkdownWorkspacePath({
    rawPath: 'temp_dir/conflict-map.png',
    ownerId: 'demo-user',
    workspaceScopeId: scopeId
  });
  assert.equal(
    resolved,
    buildWorkspacePublicPathFromScope(scopeId, 'temp_dir/conflict-map.png')
  );
});

test('maps local absolute paths back to workspace resources in desktop mode', () => {
  const resolved = resolveMarkdownWorkspacePath({
    rawPath: 'C:\\workspace\\demo-user__c__7\\temp_dir\\briefing.md',
    ownerId: 'demo-user',
    containerId: 7,
    desktopLocalMode: true,
    workspaceRoot: 'C:\\workspace'
  });
  assert.equal(resolved, '/workspaces/demo-user__c__7/temp_dir/briefing.md');
});
