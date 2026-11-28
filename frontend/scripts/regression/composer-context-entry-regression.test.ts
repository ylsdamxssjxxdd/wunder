import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// 入口分层回归（规格 §7）：大脑图标是独立的上下文入口，点击直连上下文卡片；
// 模型浮层 / 模型胶囊只负责模型选择，不再承载任何上下文占用读数。
// 本测试直接读 SFC 源码做结构断言（与仓库既有源码级回归一致），不挂载组件。

const frontendRoot = resolve(process.cwd());
const readSource = (relativePath: string): string =>
  readFileSync(resolve(frontendRoot, relativePath), 'utf8');

const composerSource = readSource('src/components/chat/ChatComposer.vue');
const popoverSource = readSource('src/components/chat/ModelPickerPopover.vue');
const phoneSheetSource = readSource('src/views/messenger/sections/PhoneModelSheet.vue');
const messengerSource = readSource('src/views/MessengerView.vue');

// 取模板里第一个匹配类名的按钮块（从 class 出现位置到下一个 </button>）。
const extractButtonBlock = (source: string, className: string): string => {
  const start = source.indexOf(className);
  assert.notEqual(start, -1, `未找到按钮 ${className}`);
  const end = source.indexOf('</button>', start);
  assert.notEqual(end, -1, `按钮 ${className} 未闭合`);
  return source.slice(start, end);
};

test('模型浮层不再展示上下文占用读数行', () => {
  assert.equal(popoverSource.includes('contextUsage'), false);
  assert.equal(popoverSource.includes('open-context'), false);
  assert.equal(popoverSource.includes('composer-model-context'), false);
  assert.equal(popoverSource.includes('stats.contextTokens'), false);
  assert.equal(popoverSource.includes('status.contextOverflow'), false);
});

test('桌面版大脑是独立按钮，点击只打开上下文卡片', () => {
  const entryBlock = extractButtonBlock(composerSource, 'composer-context-entry');
  assert.match(entryBlock, /@click\.stop="openContextCard"/);
  assert.match(entryBlock, /ContextUsageIcon/);
  // 入口文案显示当前占用百分比，而不是固定的「查看上下文」。
  assert.match(entryBlock, /:title="contextEntryTitle"/);
  assert.match(entryBlock, /:aria-label="contextEntryTitle"/);
  assert.match(composerSource, /contextEntryTitle = computed/);
  assert.ok(composerSource.includes("t('chat.contextWindow.usage')"));
  assert.ok(composerSource.includes('contextUsage.value.percentText'));

  const triggerBlock = extractButtonBlock(composerSource, 'composer-model-trigger');
  // 模型触发器内不再放大脑图标，二者热区互不覆盖。
  assert.equal(triggerBlock.includes('ContextUsageIcon'), false);
  assert.match(triggerBlock, /composer-model-name/);
});

test('桌面版模型浮层绑定不再传入占用读数或上下文事件', () => {
  assert.equal(composerSource.includes(':context-usage'), false);
  assert.equal(composerSource.includes('@open-context'), false);
});

test('手机模型面板不再接收占用读数 prop', () => {
  assert.equal(phoneSheetSource.includes('contextUsage'), false);
  assert.equal(phoneSheetSource.includes(':context-usage'), false);
});

test('手机版大脑与模型胶囊拆成两个独立热区', () => {
  const contextBlock = extractButtonBlock(messengerSource, 'messenger-mobile-head-context');
  assert.match(contextBlock, /contextCardOpen = true/);
  assert.match(contextBlock, /ContextUsageIcon/);
  assert.match(contextBlock, /:title="phoneContextEntryTitle"/);
  assert.match(contextBlock, /:aria-label="phoneContextEntryTitle"/);
  assert.match(messengerSource, /phoneContextEntryTitle = vueComputed/);
  assert.ok(messengerSource.includes("t('chat.contextWindow.usage')"));
  assert.ok(messengerSource.includes('phoneContextUsage.value.percentText'));

  const modelBlock = extractButtonBlock(messengerSource, 'messenger-mobile-head-model');
  assert.equal(modelBlock.includes('ContextUsageIcon'), false);
  assert.equal(messengerSource.includes('messenger-mobile-head-model-usage'), false);

  // 手机端同样挂载上下文卡片；面板绑定不传占用读数。
  assert.match(messengerSource, /import ContextWindowCard from/);
  assert.match(messengerSource, /<ContextWindowCard/);
  assert.equal(messengerSource.includes(':context-usage'), false);
});
