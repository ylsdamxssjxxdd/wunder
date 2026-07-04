import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const frontendRoot = resolve(process.cwd());

const readSource = (relativePath: string): string =>
  readFileSync(resolve(frontendRoot, relativePath), 'utf8').replace(/\r\n/g, '\n');

const requireInOrder = (source: string, tokens: string[]): void => {
  let cursor = -1;
  tokens.forEach((token) => {
    const next = source.indexOf(token, cursor + 1);
    assert.ok(next > cursor, `expected token after previous token: ${token}`);
    cursor = next;
  });
};

test('messenger installs renderable messages before identity and navigation derived state', () => {
  const coreState = readSource('src/views/messenger/controller/messengerControllerCoreState.ts');
  const rootInstaller = readSource('src/views/messenger/controller/installMessengerController.ts');
  const conversationRuntime = readSource('src/views/messenger/controller/messengerControllerConversationRuntime.ts');

  requireInOrder(coreState, [
    'installMessengerControllerStateRefs',
    'installMessengerControllerShellLayoutState',
    'installMessengerControllerRenderableMessages',
    'installMessengerControllerAgentIdentityState'
  ]);
  requireInOrder(rootInstaller, [
    'installMessengerControllerCoreState',
    'installMessengerControllerNavigationLists',
    'installMessengerControllerConversationRuntime'
  ]);
  assert.ok(!conversationRuntime.includes('installMessengerControllerRenderableMessages'));
});

test('messenger display-derived agent state reads the renderable source instead of the legacy message array', () => {
  const identityState = readSource('src/views/messenger/controller/messengerControllerAgentIdentityState.ts');
  const runtimeToolLists = readSource('src/views/messenger/controller/messengerControllerRuntimeToolLists.ts');
  const panelSummaries = readSource('src/views/messenger/controller/messengerControllerPanelSummaries.ts');
  const lifecycleReactive = readSource('src/views/messenger/controller/messengerControllerLifecycleReactiveEffects.ts');
  const messageCommands = readSource('src/views/messenger/controller/messengerControllerAgentMessageCommands.ts');
  const chatComposer = readSource('src/components/chat/ChatComposer.vue');
  const messengerView = readSource('src/views/MessengerView.vue');

  assert.ok(identityState.includes('messageCount: ctx.resolveActiveAgentRenderableMessageRecords().length'));
  assert.ok(identityState.includes('const messages = ctx.resolveActiveAgentRenderableMessageRecords();'));
  assert.ok(runtimeToolLists.includes('? ctx.resolveActiveAgentRenderableMessageRecords()'));
  assert.ok(panelSummaries.includes('ctx.resolveEffectiveSessionBusy(sessionId, ctx.resolveActiveAgentRenderableMessageRecords())'));
  assert.ok(panelSummaries.includes('const messages = ctx.resolveActiveAgentRenderableMessageRecords();'));
  assert.ok(panelSummaries.includes('() => ctx.resolveActiveAgentRenderableMessageRecords().length'));
  assert.ok(lifecycleReactive.includes('watch(() => [ctx.chatStore.activeSessionId, ctx.resolveActiveAgentRenderableMessageRecords().length]'));
  assert.ok(messageCommands.includes('messageCount: ctx.resolveActiveAgentRenderableMessageRecords().length'));
  assert.ok(messageCommands.includes('? ctx.resolveActiveAgentRenderableMessageRecords()'));
  assert.ok(chatComposer.includes('contextMessages:'));
  assert.ok(chatComposer.includes('Array.isArray(props.contextMessages) ? props.contextMessages : []'));
  assert.ok(chatComposer.includes('messageCount: Array.isArray(props.contextMessages) ? props.contextMessages.length : 0'));
  assert.ok(messengerView.includes(':context-messages="agentRenderableContextMessages"'));

  const forbiddenPanelReads = [
    'Array.isArray(ctx.chatStore.messages)',
    'ctx.chatStore.messages.length'
  ];
  forbiddenPanelReads.forEach((token) => {
    assert.ok(!panelSummaries.includes(token), `panel summaries should not read legacy messages: ${token}`);
  });

  const forbiddenMessageCommandReads = [
    'messageCount: Array.isArray(ctx.chatStore.messages)',
    '? (Array.isArray(ctx.chatStore.messages) ? ctx.chatStore.messages : [])',
    'Array.isArray(ctx.chatStore.messages) && ctx.chatStore.messages.length > 0'
  ];
  forbiddenMessageCommandReads.forEach((token) => {
    assert.ok(!messageCommands.includes(token), `message commands should not read legacy display messages: ${token}`);
  });

  const forbiddenComposerReads = [
    'Array.isArray(chatStore.messages) ? chatStore.messages : []',
    'messageCount: Array.isArray(chatStore.messages)'
  ];
  forbiddenComposerReads.forEach((token) => {
    assert.ok(!chatComposer.includes(token), `composer should use renderable context messages: ${token}`);
  });
});

test('messenger exposes a single active agent renderable record helper without legacy fallback', () => {
  const renderableMessages = readSource('src/views/messenger/controller/messengerControllerRenderableMessages.ts');
  const helperStart = renderableMessages.indexOf('ctx.resolveActiveAgentRenderableMessageRecords = ()');
  assert.ok(helperStart >= 0);
  const helperEnd = renderableMessages.indexOf('ctx.buildWorkflowSurfaceDebugSnapshot', helperStart);
  assert.ok(helperEnd > helperStart);
  const helperSource = renderableMessages.slice(helperStart, helperEnd);

  assert.ok(helperSource.includes('const renderable = ctx.agentRenderableMessages?.value;'));
  assert.ok(helperSource.includes('return [];'));
  assert.ok(!helperSource.includes('ctx.chatStore.messages'));
  assert.ok(renderableMessages.includes('ctx.agentRenderableContextMessages = computed<Record<string, unknown>[]>(() => {'));
  assert.ok(renderableMessages.includes('const records = ctx.resolveActiveAgentRenderableMessageRecords();'));
  assert.ok(renderableMessages.includes('const resolveSyntheticGreetingRenderable = ()'));
  assert.ok(!renderableMessages.includes('const buildLegacyAgentRenderableMessages = ()'));
});

test('projection render source has an explicit batched invalidation clock', () => {
  const chatStore = readSource('src/stores/chat.ts');
  const runtimeState = readSource('src/stores/chatRuntimeState.ts');
  const invalidation = readSource('src/realtime/chat/chatRuntimeProjectionInvalidation.ts');
  const renderableMessages = readSource('src/views/messenger/controller/messengerControllerRenderableMessages.ts');
  const lifecycleReactive = readSource('src/views/messenger/controller/messengerControllerLifecycleReactiveEffects.ts');

  assert.ok(chatStore.includes('runtimeProjectionVersion: 0'));
  assert.ok(chatStore.includes('const _projectionVersion = state.runtimeProjectionVersion;'));
  assert.ok(runtimeState.includes('markRuntimeProjectionChanged'));
  assert.ok(runtimeState.includes('applyChatRuntimeEventsWithInvalidation'));
  assert.ok(invalidation.includes('export const markRuntimeProjectionChanged = ('));
  assert.ok(invalidation.includes('requestAnimationFrame'));
  assert.ok(invalidation.includes('globalThis.setTimeout(() => bump(), 16)'));
  assert.ok(runtimeState.includes('markRuntimeProjectionChanged(store, {'));
  assert.ok(renderableMessages.includes('const _projectionRenderVersion = ctx.chatStore.runtimeProjectionVersion;'));
  assert.ok(renderableMessages.includes('const projection = toRaw(ctx.chatStore.runtimeProjection);'));
  assert.ok(renderableMessages.includes('projection,'));
  assert.ok(renderableMessages.includes("logAgentRenderSource('projection-source'"));
  assert.ok(!renderableMessages.includes('projection-empty-fallback'));
  assert.ok(!renderableMessages.includes('return legacyRenderable;'));
  assert.ok(lifecycleReactive.includes('ctx.chatStore.runtimeProjectionVersion'));
  assert.ok(!lifecycleReactive.includes('ctx.chatStore.messageMutationVersion'));
});

test('store visibleMessages getter materializes projection without legacy raw fallback', () => {
  const chatStore = readSource('src/stores/chat.ts');
  const watcher = readSource('src/stores/chatWatcher.ts');
  const renderAdapter = readSource('src/realtime/chat/chatRuntimeRenderAdapter.ts');
  const sessionOpenLoadActions = readSource('src/stores/chatSessionOpenLoadActions.ts');

  assert.ok(chatStore.includes('return materializeChatRuntimeMessages(state.runtimeProjection, sessionId || state.activeSessionId);'));
  assert.ok(sessionOpenLoadActions.includes('applyLocalChatMessageRuntimeEvent(this, {'));
  assert.ok(!chatStore.includes('resolveProjectedVisibleMessagesFromStore'));
  assert.ok(!chatStore.includes('messageRuntimeStatus:'));
  assert.ok(!chatStore.includes('resolveLegacyMessageRuntimeStatusFromStore'));
  assert.ok(!watcher.includes('export const resolveProjectedVisibleMessagesFromStore'));
  assert.ok(!watcher.includes('export const resolveLegacyMessageRuntimeStatusFromStore'));
  assert.ok(!watcher.includes('const byRaw = new Map();'));
  assert.ok(!watcher.includes('return sourceMessages;'));
  assert.ok(renderAdapter.includes('const base: ChatMessageLike = cloneDisplayProjection(message.display);'));
  assert.ok(renderAdapter.includes('isSyntheticGreetingDisplay(message.display)'));
  assert.ok(!renderAdapter.includes('message.raw'));
  assert.ok(!renderAdapter.includes('raw ? { ...raw } : {}'));
  assert.ok(!renderAdapter.includes('__runtime_raw_message'));
});
