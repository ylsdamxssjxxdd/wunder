import test from 'node:test';
import assert from 'node:assert/strict';

import {
  resolveAbilityVisual,
  resolveToolIconClass
} from '../../src/utils/abilityVisuals';

test('tool icon inference ignores generic search wording in names and descriptions', () => {
  assert.equal(
    resolveToolIconClass({
      name: 'generic ability',
      description: 'description mentions \u68c0\u7d22 wording'
    }),
    'fa-toolbox'
  );

  assert.equal(
    resolveToolIconClass({
      name: 'generic query wording',
      description: 'search and retrieve are regular wording here'
    }),
    'fa-toolbox'
  );
});

test('tool icon inference keeps explicit search tool identifiers', () => {
  assert.equal(resolveToolIconClass({ name: 'web_search' }), 'fa-magnifying-glass');
  assert.equal(resolveToolIconClass({ name: 'search_content' }), 'fa-magnifying-glass');
});

test('ability visuals keep skill and knowledge defaults when descriptions mention retrieval', () => {
  assert.deepEqual(
    resolveAbilityVisual({
      kind: 'skill',
      group: 'skill',
      name: 'generic skill',
      description: 'description mentions \u68c0\u7d22 wording'
    }),
    {
      icon: 'fa-book',
      tone: 'skill'
    }
  );

  assert.deepEqual(
    resolveAbilityVisual({
      group: 'knowledge',
      source: 'user_knowledge',
      name: 'generic knowledge',
      description: 'description mentions \u68c0\u7d22 wording'
    }),
    {
      icon: 'fa-database',
      tone: 'knowledge'
    }
  );
});

test('ability visuals keep MCP sources on plug icon over skill-like wording', () => {
  assert.deepEqual(
    resolveAbilityVisual({
      group: 'mcp',
      source: 'mcp',
      name: 'template_read',
      description: 'Read presentation template metadata for generated pages.'
    }),
    {
      icon: 'fa-plug',
      tone: 'mcp'
    }
  );

  assert.equal(
    resolveToolIconClass({
      group: 'user-mcp',
      source: 'user_mcp',
      name: 'prompt_template_query',
      description: 'Query knowledge documents through a prompt template.'
    }),
    'fa-plug'
  );
});


test('command session uses the terminal icon in both localized and runtime names', () => {
  assert.equal(resolveToolIconClass({ name: '命令会话' }), 'fa-terminal');
  assert.equal(resolveToolIconClass({ name: 'command_session' }), 'fa-terminal');
});

test('glob tool uses the folder tree icon across canonical and runtime names', () => {
  for (const name of ['文件名匹配', 'glob', 'glob_files']) {
    assert.equal(resolveToolIconClass({ name }), 'fa-folder-tree');
    assert.deepEqual(resolveAbilityVisual({ name, kind: 'tool' }), {
      icon: 'fa-folder-tree',
      tone: 'file'
    });
  }
  assert.deepEqual(resolveAbilityVisual({ name: '文件名通配匹配', kind: 'tool' }), {
    icon: 'fa-folder-tree',
    tone: 'file'
  });
});

test('read file ignores read_image wording in its description', () => {
  const description =
    '以有界、面向任务的方式读取本地纯文本 UTF-8 文件。仅限文本文件；不支持 docx/xlsx，图片请用 read_image。';
  assert.deepEqual(resolveAbilityVisual({ name: '读取文件', description, kind: 'tool' }), {
    icon: 'fa-file-lines',
    tone: 'file'
  });
  assert.deepEqual(resolveAbilityVisual({ name: '读图工具', description, kind: 'tool' }), {
    icon: 'fa-eye',
    tone: 'search'
  });
});

test('canonical builtin titles share the desktop and Android glyph table', () => {
  const cases: Array<[string, string, string]> = [
    ['执行命令', 'fa-terminal', 'terminal'],
    ['命令会话', 'fa-terminal', 'terminal'],
    ['读取文件', 'fa-file-lines', 'file'],
    ['列出文件', 'fa-folder-open', 'file'],
    ['写入文件', 'fa-file-circle-plus', 'file'],
    ['编辑', 'fa-file-pen', 'file'],
    ['应用补丁', 'fa-pen-to-square', 'file'],
    ['文件名匹配', 'fa-folder-tree', 'file'],
    ['网页抓取', 'fa-globe', 'search'],
    ['网页搜索', 'fa-magnifying-glass', 'general'],
    ['浏览器', 'fa-window-maximize', 'search'],
    ['读图工具', 'fa-eye', 'search'],
    ['图像生成', 'fa-paintbrush', 'search'],
    ['桌面控制', 'fa-computer-mouse', 'general'],
    ['手机控制', 'fa-mobile-screen', 'general'],
    ['安卓屏幕', 'fa-display', 'general'],
    ['安卓通知', 'fa-bell', 'general'],
    ['计划任务', 'fa-clock', 'automation'],
    ['技能调用', 'fa-book-open', 'skill']
  ];
  for (const [name, icon, tone] of cases) {
    assert.deepEqual(resolveAbilityVisual({ name, kind: 'tool' }), { icon, tone }, name);
    assert.equal(resolveToolIconClass({ name }), icon, name);
  }
});

test('web fetch keeps the globe even when its description mentions browser', () => {
  const description =
    '抓取单个公开网页 URL 并提取正文。登录、注册、填表或点击等交互任务直接使用 browser 的 action=navigate，不先抓取页面。';
  assert.deepEqual(resolveAbilityVisual({ name: '网页抓取', description, kind: 'tool' }), {
    icon: 'fa-globe',
    tone: 'search'
  });
  assert.deepEqual(resolveAbilityVisual({ name: '浏览器', description, kind: 'tool' }), {
    icon: 'fa-window-maximize',
    tone: 'search'
  });
});
