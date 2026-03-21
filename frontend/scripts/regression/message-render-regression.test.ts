import test from 'node:test';
import assert from 'node:assert/strict';

import { prepareMessageMarkdownContent } from '../../src/utils/messageMarkdown';
import {
  buildAgentWorkspaceScopeId,
  buildWorkspacePublicPathFromScope,
  resolveMarkdownWorkspacePath
} from '../../src/utils/messageWorkspacePath';
import { buildAssistantDisplayContent } from '../../src/utils/assistantFailureNotice';

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

const failureNoticeMessages: Record<string, string> = {
  'chat.message.failedInlineTitle': '本次回复未完成',
  'chat.message.failedInlineReason': '错误原因：{detail}',
  'chat.message.failedInlinePartial': '以下内容是失败前已生成的部分输出，仅供参考。',
  'chat.workflow.aborted': '已中止',
  'chat.workflow.abortedDetail': '本次请求已中止',
  'chat.workflow.requestFailed': '请求失败',
  'chat.workflow.error': '错误',
  'chat.workflow.requestFailedDetail': '请求失败，请稍后重试'
};

const failureNoticeTranslator = (key: string, named?: Record<string, unknown>): string => {
  const template = failureNoticeMessages[key] || key;
  return template.replace(/\{(\w+)\}/g, (_token, name: string) => String(named?.[name] ?? ''));
};

test('omits partial block when assistant content is only the same failure detail', () => {
  const detail =
    '模型调用失败: LLM stream request failed: 429 Too Many Requests {"error":{"message":"quota exceeded"}}';
  const rendered = buildAssistantDisplayContent(
    {
      role: 'assistant',
      content: detail,
      workflowItems: [{ status: 'failed', detail }]
    },
    failureNoticeTranslator
  );
  assert.equal(rendered, `**⚠️ 本次回复未完成**\n\n错误原因：${detail}`);
});

test('keeps partial block but trims duplicated trailing failure line', () => {
  const detail = '模型调用失败: LLM stream request failed: 429 Too Many Requests';
  const rendered = buildAssistantDisplayContent(
    {
      role: 'assistant',
      content: `先给你一份摘要。\n${detail}`,
      workflowItems: [{ status: 'failed', detail }]
    },
    failureNoticeTranslator
  );
  assert.equal(
    rendered,
    [
      '**⚠️ 本次回复未完成**',
      '',
      `错误原因：${detail}`,
      '',
      '以下内容是失败前已生成的部分输出，仅供参考。',
      '',
      '---',
      '',
      '先给你一份摘要。'
    ].join('\n')
  );
});
