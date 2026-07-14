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

test('message workflow component keeps a stable key across live tool updates', () => {
  const messengerView = readSource('src/views/MessengerView.vue');
  const workflowComponent = readSource('src/components/chat/MessageToolWorkflow.vue');
  const workflowStart = messengerView.indexOf('<MessageToolWorkflow');
  assert.ok(workflowStart >= 0);
  const workflowEnd = messengerView.indexOf('@layout-change', workflowStart);
  assert.ok(workflowEnd > workflowStart);
  const workflowSource = messengerView.slice(workflowStart, workflowEnd);

  assert.ok(workflowSource.includes(':key="`workflow:${item.key}`"'));
  assert.ok(!workflowSource.includes(':key="`workflow:${item.key}:${buildMessageWorkflowRenderVersion'));
  assert.ok(workflowSource.includes(':render-version="buildMessageWorkflowRenderVersion(item.message)"'));
  assert.ok(workflowSource.includes(':state-key="`${sessionHub.activeConversationKey}:workflow:${resolveMessageWorkflowStateKey(item.message, item.sourceIndex)}`"'));
  assert.ok(workflowSource.includes(':state-aliases="resolveMessageWorkflowStateAliases(item.message, item.sourceIndex, item.key)'));
  assert.ok(workflowSource.includes('.map((key) => `${sessionHub.activeConversationKey}:workflow:${key}`)"'));
  assert.ok(workflowComponent.includes('<script lang="ts">'));
  assert.ok(workflowComponent.includes('const workflowStateCache = new Map<string, WorkflowPanelState>();'));
  assert.ok(workflowComponent.includes('let workflowStateCacheClock = 0;'));
  assert.ok(workflowComponent.includes('stateAliases?: string[];'));
  assert.ok(workflowComponent.includes('updatedAt: ++workflowStateCacheClock'));
  assert.ok(workflowComponent.includes('normalizeWorkflowStateKeys(props.stateKey, props.stateAliases)'));
  assert.ok(workflowComponent.includes('restoreWorkflowPanelState(props.stateKey, props.stateAliases);'));
  assert.ok(workflowComponent.includes('stateUpdatedAt > cachedUpdatedAt'));
  assert.ok(workflowComponent.includes('workflowStateCache.set(key, cached);'));
  assert.ok(workflowComponent.includes('saveWorkflowPanelState();\n  clearToolCallDebugHintHideTimer();'));
  assert.ok(!workflowComponent.includes('if (liveKey && !nextUserCollapsed.has(liveKey))'));
  assert.ok(workflowComponent.includes('if (validKeys.has(key)) nextExpanded.add(key);'));
  assert.ok(workflowComponent.includes('if (!workflowOpen.value) return;'));
  assert.ok(workflowComponent.includes('const WORKFLOW_EXPANDED_ENTRY_LIMIT = 3;'));
  assert.ok(workflowComponent.includes('const limitExpandedKeys = (keys: Iterable<string>): Set<string> => {'));
  assert.ok(workflowComponent.includes('v-if="expandedKeys.has(entry.key)"'));

  const routingPreferences = readSource('src/views/messenger/controller/messengerControllerMessageRoutingPreferences.ts');
  assert.ok(routingPreferences.includes('ctx.resolveMessageWorkflowStateKey ='));
  assert.ok(routingPreferences.includes('message?.__runtime_message_id'));
  assert.ok(routingPreferences.includes("message?.__runtime_model_turn_id"));
  assert.ok(routingPreferences.includes("message?.model_turn_id"));
  assert.ok(routingPreferences.includes('ctx.resolveMessageWorkflowStateAliases ='));
  assert.ok(routingPreferences.includes('`workflow-model-turn:${modelTurnId}`'));
  assert.ok(routingPreferences.includes('`workflow-first-item:${firstWorkflowRef}`'));
  assert.ok(routingPreferences.includes('toolCallRawDetail || item?.tool_call_raw_detail'));
  assert.ok(routingPreferences.includes('context_occupancy_tokens'));
});

test('active agent plan panel preserves user expansion across streaming projection churn', () => {
  const messengerView = readSource('src/views/MessengerView.vue');
  const stateRefs = readSource('src/views/messenger/controller/messengerControllerStateRefs.ts');
  const renderableMessages = readSource('src/views/messenger/controller/messengerControllerRenderableMessages.ts');
  const reactiveEffects = readSource('src/views/messenger/controller/messengerControllerLifecycleReactiveEffects.ts');

  assert.ok(messengerView.includes(':key="activeAgentPlanKey || \'active-agent-plan\'"'));
  assert.ok(messengerView.includes('const activeAgentPlanKey = controller.activeAgentPlanKey;'));
  assert.ok(stateRefs.includes('ctx.agentPlanExpandedByKey = new Map<string, boolean>();'));
  assert.ok(renderableMessages.includes('ctx.resolveAgentPlanPanelKey ='));
  assert.ok(renderableMessages.includes('ctx.activeAgentPlanKey = computed(() => ctx.resolveAgentPlanPanelKey(ctx.activeAgentPlanMessage.value));'));
  assert.ok(reactiveEffects.includes('const rememberAgentPlanExpandedState ='));
  assert.ok(reactiveEffects.includes('while (ctx.agentPlanExpandedByKey.size > 120)'));
  assert.ok(reactiveEffects.includes('watch(() => ctx.activeAgentPlanKey.value'));
  assert.ok(reactiveEffects.includes("const activeConversationPrefix = `${String(ctx.sessionHub.activeConversationKey || '')}:`;"));
  assert.ok(reactiveEffects.includes('if (oldKey && oldKey.startsWith(activeConversationPrefix))'));
  assert.ok(reactiveEffects.includes('rememberAgentPlanExpandedState(oldKey, ctx.agentPlanExpanded.value);'));
  assert.ok(!reactiveEffects.includes('watch(() => ctx.activeAgentPlan.value, (value) => {\n      if (!value) {\n          ctx.agentPlanExpanded.value = false;'));
});

test('messenger interaction blocker leaves the right workspace dock interactive', () => {
  const styles = readSource('src/styles/messenger.css');
  const blockerStart = styles.indexOf('.messenger-action-blocker {');
  assert.ok(blockerStart >= 0);
  const blockerEnd = styles.indexOf('.messenger-action-blocker-card', blockerStart);
  assert.ok(blockerEnd > blockerStart);
  const blockerSource = styles.slice(blockerStart, blockerEnd);

  assert.ok(!blockerSource.includes('inset: 0;'));
  assert.ok(blockerSource.includes('left: calc(var(--messenger-left-rail-width) + var(--messenger-middle-pane-width));'));
  assert.ok(blockerSource.includes('right: var(--messenger-right-dock-width);'));
  assert.ok(blockerSource.includes('.messenger-view.messenger-view--without-right .messenger-action-blocker'));
  assert.ok(blockerSource.includes('.messenger-view.messenger-view--right-collapsed .messenger-action-blocker'));
});

test('workspace resource hydration bounds inactive object URL cache entries', () => {
  const hydration = readSource('src/views/messenger/controller/messengerControllerWorkspaceResourceHydration.ts');

  assert.ok(hydration.includes('const WORKSPACE_RESOURCE_CACHE_LIMIT = 48;'));
  assert.ok(hydration.includes('const collectActiveWorkspaceObjectUrls = (): Set<string> => {'));
  assert.ok(hydration.includes("document.querySelectorAll<HTMLImageElement>('img[src^=\"blob:\"]')"));
  assert.ok(hydration.includes('const pruneWorkspaceResourceCache = () => {'));
  assert.ok(hydration.includes('activeUrls.has(objectUrl) || entry?.promise'));
  assert.ok(hydration.includes('pruneWorkspaceResourceCache();'));
  assert.ok(hydration.includes('abortWorkspaceResourceRequests();'));
});

test('streaming projection changes only refresh the latest message layout', () => {
  const reactiveEffects = readSource('src/views/messenger/controller/messengerControllerLifecycleReactiveEffects.ts');
  const renderableMessages = readSource('src/views/messenger/controller/messengerControllerRenderableMessages.ts');

  const structureWatchStart = reactiveEffects.indexOf("reason: 'message-structure-change'");
  assert.ok(structureWatchStart >= 0);
  const structureWatchSource = reactiveEffects.slice(
    Math.max(0, structureWatchStart - 700),
    structureWatchStart + 260
  );
  assert.ok(!structureWatchSource.includes('ctx.chatStore.runtimeProjectionVersion'));

  const latestWatchStart = reactiveEffects.indexOf("ctx.refreshLatestAssistantMessageLayout('latest-assistant-signature')");
  assert.ok(latestWatchStart >= 0);
  const latestWatchSource = reactiveEffects.slice(
    Math.max(0, latestWatchStart - 520),
    latestWatchStart + 160
  );
  assert.ok(!latestWatchSource.includes('ctx.chatStore.runtimeProjectionVersion'));
  assert.ok(!latestWatchSource.includes('scheduleWorkspaceResourceHydration'));

  const messageViewport = readSource('src/views/messenger/controller/messengerControllerLifecycleMessageViewport.ts');
  const markdownRenderedStart = messageViewport.indexOf('ctx.handleMessageMarkdownRendered =');
  assert.ok(markdownRenderedStart >= 0);
  const markdownRenderedEnd = messageViewport.indexOf('ctx.updateMessageScrollState =', markdownRenderedStart);
  assert.ok(markdownRenderedEnd > markdownRenderedStart);
  const markdownRenderedSource = messageViewport.slice(markdownRenderedStart, markdownRenderedEnd);
  assert.ok(markdownRenderedSource.includes("reason: payload.streaming ? 'streaming-markdown-rendered' : 'markdown-rendered'"));
  assert.ok(markdownRenderedSource.includes('const lightweightStreaming = payload.streaming === true && payload.lightweight === true;'));
  assert.ok(markdownRenderedSource.includes('if (!lightweightStreaming)'));
  assert.ok(markdownRenderedSource.includes('payload.needsHydration === true'));
  assert.ok(markdownRenderedSource.includes("messageKeys: [normalizedKey]"));

  const layoutSignatureStart = renderableMessages.indexOf('ctx.buildLatestAssistantLayoutSignature =');
  assert.ok(layoutSignatureStart >= 0);
  const layoutSignatureEnd = renderableMessages.indexOf('ctx.latestWorldRenderableMessageKey =', layoutSignatureStart);
  assert.ok(layoutSignatureEnd > layoutSignatureStart);
  const layoutSignatureSource = renderableMessages.slice(layoutSignatureStart, layoutSignatureEnd);
  assert.ok(!layoutSignatureSource.includes('runtimeProjectionVersion'));

  const statsStart = renderableMessages.indexOf('ctx.buildMessageStatsEntries =');
  assert.ok(statsStart >= 0);
  const statsEnd = renderableMessages.indexOf('ctx.shouldShowMessageStats =', statsStart);
  assert.ok(statsEnd > statsStart);
  const statsSource = renderableMessages.slice(statsStart, statsEnd);
  assert.ok(!statsSource.includes('ctx.chatStore.runtimeProjectionVersion'));
});

test('message panel keeps actions quiet and preserves virtual rendering outside chat section', () => {
  const messengerView = readSource('src/views/MessengerView.vue');
  const renderableMessages = readSource('src/views/messenger/controller/messengerControllerRenderableMessages.ts');
  const viewportRuntime = readSource('src/views/messenger/messageViewportRuntime.ts');
  const lifecycleReactive = readSource('src/views/messenger/controller/messengerControllerLifecycleReactiveEffects.ts');
  const styles = readSource('src/styles/messenger.css');

  assert.ok(messengerView.includes('v-show="retainedMessageRenderKind === \'agent\'"'));
  assert.ok(messengerView.includes('v-show="retainedMessageRenderKind === \'world\'"'));
  assert.ok(messengerView.includes('const retainedMessageRenderKind = controller.retainedMessageRenderKind;'));
  assert.ok(renderableMessages.includes('ctx.retainedMessageRenderKind = computed<MessageConversationKind>(() => {'));
  assert.ok(renderableMessages.includes("ctx.retainedMessageRenderKind?.value === 'world'"));

  const virtualizationStart = renderableMessages.indexOf('ctx.shouldVirtualizeMessages = computed(() => {');
  assert.ok(virtualizationStart >= 0);
  const virtualizationEnd = renderableMessages.indexOf('ctx.resolveVirtualMessageHeight =', virtualizationStart);
  assert.ok(virtualizationEnd > virtualizationStart);
  const virtualizationSource = renderableMessages.slice(virtualizationStart, virtualizationEnd);
  assert.ok(!virtualizationSource.includes("ctx.sessionHub.activeSection !== 'messages'"));
  assert.ok(!virtualizationSource.includes('ctx.showChatSettingsView.value'));

  assert.ok(viewportRuntime.includes('restoreConversationScroll = async (restoreOptions: { deferMeasure?: boolean } = {})'));
  assert.ok(viewportRuntime.includes("scheduleDeferredVisibleMeasure('restore-conversation-scroll')"));
  assert.ok(lifecycleReactive.includes('ctx.restoreConversationScroll?.({ deferMeasure: true })'));

  const actionButtonStart = styles.indexOf('.messenger-message-footer-copy {');
  assert.ok(actionButtonStart >= 0);
  const actionButtonEnd = styles.indexOf('.messenger-message-footer-copy:hover', actionButtonStart);
  assert.ok(actionButtonEnd > actionButtonStart);
  const actionButtonSource = styles.slice(actionButtonStart, actionButtonEnd);
  assert.ok(actionButtonSource.includes('opacity: 0;'));
  assert.ok(actionButtonSource.includes('pointer-events: none;'));
  assert.ok(styles.includes('.messenger-message:hover .messenger-message-footer-copy'));
  assert.ok(styles.includes('.messenger-message:focus-within .messenger-message-footer-copy'));
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
  assert.ok(chatStore.includes('runtimeProjectionContentVersion: 0'));
  assert.ok(chatStore.includes('runtimeProjectionContentVersionByMessage: {} as Record<string, number>'));
  assert.ok(chatStore.includes('const _projectionVersion = state.runtimeProjectionVersion;'));
  assert.ok(runtimeState.includes('markRuntimeProjectionChanged'));
  assert.ok(runtimeState.includes('applyChatRuntimeEventsWithInvalidation'));
  assert.ok(invalidation.includes('export const markRuntimeProjectionChanged = ('));
  assert.ok(invalidation.includes('runtimeProjectionContentVersionByMessage'));
  assert.ok(invalidation.includes('contentOnlyResults.length === appliedResults.length'));
  assert.ok(invalidation.includes('requestAnimationFrame'));
  assert.ok(invalidation.includes('DEFAULT_PROJECTION_INVALIDATION_DELAY_MS = 24'));
  assert.ok(invalidation.includes('lastBumpedAt'));
  assert.ok(invalidation.includes('Math.max(16, delayMs)'));
  assert.ok(runtimeState.includes('markRuntimeProjectionChanged(store, {'));
  assert.ok(renderableMessages.includes('const _projectionRenderVersion = ctx.chatStore.runtimeProjectionVersion;'));
  assert.ok(renderableMessages.includes('const projection = toRaw(ctx.chatStore.runtimeProjection);'));
  assert.ok(renderableMessages.includes('projection,'));
  assert.ok(renderableMessages.includes("logAgentRenderSource('projection-source'"));
  assert.ok(!renderableMessages.includes('projection-empty-fallback'));
  assert.ok(!renderableMessages.includes('return legacyRenderable;'));
  assert.ok(!lifecycleReactive.includes('ctx.chatStore.messageMutationVersion'));
});

test('realtime pulse does not refresh the full session list during interactive send streams', () => {
  const runtimeMeta = readSource('src/views/messenger/controller/messengerControllerLifecycleRuntimeMeta.ts');
  const sessionOpenLoadActions = readSource('src/stores/chatSessionOpenLoadActions.ts');

  assert.ok(runtimeMeta.includes('ctx.isActiveChatInteractiveStream = () => {'));
  assert.ok(runtimeMeta.includes('runtime?.sendController || runtime?.resumeController'));
  assert.ok(runtimeMeta.includes("session-refresh-skip-interactive-stream"));
  assert.ok(runtimeMeta.includes('buildRuntimeDebugSnapshot(getRuntime(ctx.chatStore.activeSessionId))'));
  assert.ok(sessionOpenLoadActions.includes('const activeRuntimeInteractive = Boolean('));
  assert.ok(sessionOpenLoadActions.includes("traceSource === 'realtime-pulse'"));
  assert.ok(sessionOpenLoadActions.includes('activeRuntimeInteractive'));
  assert.ok(sessionOpenLoadActions.includes("load-sessions-skip-interactive-stream"));
});

test('streaming message text updates are scoped to the markdown body component', () => {
  const messengerView = readSource('src/views/MessengerView.vue');
  const markdownBody = readSource('src/components/chat/MessageMarkdownBody.vue');
  const renderAdapter = readSource('src/realtime/chat/chatRuntimeRenderAdapter.ts');
  const renderableController = readSource('src/views/messenger/controller/messengerControllerRenderableMessages.ts');
  const companionFloatingLayer = readSource('src/components/companions/CompanionFloatingLayer.vue');

  assert.ok(messengerView.includes(':runtime-message-id="String(item.message.__runtime_message_id || item.message.message_id || \'\')"'));
  assert.ok(messengerView.includes(':runtime-user-turn-id="String(item.message.__runtime_user_turn_id || item.message.user_turn_id || item.message.userTurnId || \'\')"'));
  assert.ok(messengerView.includes(':runtime-model-turn-id="String(item.message.__runtime_model_turn_id || item.message.model_turn_id || item.message.modelTurnId || \'\')"'));
  assert.ok(messengerView.includes(':session-id="String(chatStore.activeSessionId || \'\')"'));
  assert.ok(messengerView.includes('shouldMountAgentMessageBubble(item.message)'));
  assert.ok(markdownBody.includes('selectLatestAssistantForTurn'));
  assert.ok(markdownBody.includes('resolveRuntimeContentSubscriptionMessageIds'));
  assert.ok(markdownBody.includes('props.runtimeUserTurnId'));
  assert.ok(markdownBody.includes('props.runtimeModelTurnId'));
  assert.ok(!markdownBody.includes('runtimeProjectionVersion'));
  assert.ok(markdownBody.includes('runtimeContentVersion.value'));
  assert.ok(markdownBody.includes('const turnMessage = resolveRuntimeProjectedMessageByTurn(projection, sessionId);'));
  assert.ok(markdownBody.includes('runtimeProjectionContentVersionByMessage?.[messageId]'));
  assert.ok(markdownBody.includes('const resolveRuntimeProjectedMessage = () => {'));
  assert.ok(markdownBody.includes('const _contentVersion = runtimeContentVersion.value;'));
  assert.ok(markdownBody.includes('const projected = resolveRuntimeProjectedMessage();'));
  assert.ok(markdownBody.includes("chatDebugLog('chat.stream.perf', 'message-body-stream-render'"));
  assert.ok(markdownBody.includes('const isStreamingTextPreview = computed(() =>'));
  assert.ok(markdownBody.includes('runtimeProjectionContentVersion || 0'));
  assert.ok(markdownBody.includes('ref="plainTextRef"'));
  assert.ok(markdownBody.includes('syncPlainTextDom(source);'));
  assert.ok(markdownBody.includes('LIVE_STREAM_TEXT_POLL_MS'));
  assert.ok(markdownBody.includes('syncLiveRuntimePlainText'));
  assert.ok(markdownBody.includes('if (!isChatDebugEnabled() || !props.streaming'));
  assert.ok(!markdownBody.includes('{{ visiblePlainText }}'));
  assert.ok(markdownBody.includes('STREAMING_TEXT_PREVIEW_MAX_CHARS'));
  assert.ok(markdownBody.includes('props.streaming === true'));
  assert.ok(markdownBody.includes('? isStreamingTextPreview.value'));
  assert.ok(markdownBody.includes('STREAM_TEXT_FLUSH_MIN_MS'));
  assert.ok(markdownBody.includes('const HISTORY_MARKDOWN_INITIAL_CHARS = 24000;'));
  assert.ok(markdownBody.includes('const MARKDOWN_BODY_CACHE_MAX_BYTES = 12 * 1024 * 1024;'));
  assert.ok(markdownBody.includes('const writeMarkdownCacheEntry ='));
  assert.ok(markdownBody.includes('v-if="isContentTruncated"'));
  assert.ok(markdownBody.includes('getSessionHistoryMessage'));
  assert.ok(markdownBody.includes('HYDRATED_HISTORY_CONTENT_CACHE_LIMIT = 64'));
  assert.ok(markdownBody.includes('HYDRATED_HISTORY_CONTENT_CACHE_MAX_BYTES = 8 * 1024 * 1024'));
  assert.ok(markdownBody.includes("emit('history-message-hydrated'"));
  assert.ok(messengerView.includes('item.message.workflowItems_truncated === true'));
  assert.ok(messengerView.includes('item.message.subagents_truncated === true'));
  assert.ok(messengerView.includes('@history-message-hydrated="Object.assign(item.message, $event, {'));
  assert.ok(messengerView.includes('content_truncated: false,'));
  assert.ok(messengerView.includes('reasoning_truncated: false,'));
  assert.ok(messengerView.includes('workflowItems_truncated: false,'));
  assert.ok(messengerView.includes('subagents_truncated: false'));
  assert.ok(renderableController.includes('ctx.shouldMountAgentMessageBubble ='));
  assert.ok(renderableController.includes('ctx.shouldMountAgentMessageBubble = (message: Record<string, unknown>): boolean => ctx.shouldShowAgentMessageBubble(message);'));
  assert.ok(!renderableController.includes("runtimeStatus === 'tooling'"));
  assert.ok(!renderableController.includes("runtimeStatus === 'streaming'"));
  assert.ok(!renderableController.includes("if (runtimeStatus === 'streaming') {\n          return true;"));

  const revisionStart = renderAdapter.indexOf('const buildProjectionMessageMaterializationRevision =');
  assert.ok(revisionStart >= 0);
  const revisionEnd = renderAdapter.indexOf('const buildProjectionMetadataRevision =', revisionStart);
  assert.ok(revisionEnd > revisionStart);
  const revisionSource = renderAdapter.slice(revisionStart, revisionEnd);
  assert.ok(!revisionSource.includes('message.content'));
  assert.ok(!revisionSource.includes('message.reasoning'));
  assert.ok(!revisionSource.includes('message.updatedSeq'));
  assert.ok(renderAdapter.includes('syncMaterializedStreamingFields(cached.message, message);'));
  assert.ok(!renderableController.includes('runtimeProjectionContentVersion;'));

  const bubbleStart = companionFloatingLayer.indexOf('const latestActiveAssistantBubble = computed');
  assert.ok(bubbleStart >= 0);
  const bubbleEnd = companionFloatingLayer.indexOf('const allVisibleEntries = computed', bubbleStart);
  assert.ok(bubbleEnd > bubbleStart);
  const bubbleSource = companionFloatingLayer.slice(bubbleStart, bubbleEnd);
  assert.ok(bubbleSource.includes('selectVisibleMessageProjections(toRaw(chatStore.runtimeProjection), sessionId)'));
  assert.ok(!bubbleSource.includes('runtimeProjectionContentVersion'));
  assert.ok(!bubbleSource.includes('chatStore.messages'));
});

test('streaming text performance breadcrumbs are available behind debug and perf switches', () => {
  const markdownBody = readSource('src/components/chat/MessageMarkdownBody.vue');
  const invalidation = readSource('src/realtime/chat/chatRuntimeProjectionInvalidation.ts');
  const chatDebug = readSource('src/utils/chatDebug.ts');
  const companionSprite = readSource('src/components/companions/CompanionSprite.vue');

  assert.ok(markdownBody.includes("chatDebugLog('chat.stream.perf', 'plain-text-slow-flush'"));
  assert.ok(markdownBody.includes("chatDebugLog('chat.stream.perf', 'markdown-slow-render'"));
  assert.ok(markdownBody.includes("chatDebugLog('chat.stream.perf', 'message-body-stream-render'"));
  assert.ok(markdownBody.includes("chatPerf.recordDuration('chat_stream_plain_text_slow_flush'"));
  assert.ok(markdownBody.includes("chatPerf.recordDuration('chat_stream_markdown_slow_render'"));
  assert.ok(markdownBody.includes('visiblePlainText.value = source;'));
  assert.ok(markdownBody.includes('plainTextRef.value'));
  assert.ok(markdownBody.includes('traceStreamingRenderSource(source, plainTextRender);'));
  assert.ok(markdownBody.includes('PLAIN_TEXT_LAYOUT_THROTTLE_MIN_MS'));
  assert.ok(markdownBody.includes('lightweight'));
  assert.ok(invalidation.includes("chatDebugLog('chat.stream.perf', 'content-clock-slow-flush'"));
  assert.ok(invalidation.includes("chatPerf.recordDuration('chat_stream_content_clock_slow_flush'"));
  assert.ok(invalidation.includes('slowFlushCount'));
  assert.ok(invalidation.includes('DEFAULT_PROJECTION_CONTENT_INVALIDATION_DELAY_MS = 24'));
  assert.ok(chatDebug.includes("const DEBUG_HISTORY_ONLY_SCOPES = new Set(["));
  assert.ok(chatDebug.includes("'chat.stream.perf'"));
  assert.ok(chatDebug.includes('const DEBUG_HEAVY_CONSOLE_SCOPES = new Set(['));
  assert.ok(chatDebug.includes('buildDebugPayloadOmissionMeta'));
  assert.ok(chatDebug.includes('if (DEBUG_HISTORY_ONLY_SCOPES.has(normalizedScope)) return;'));
  assert.ok(companionSprite.includes('animation: props.paused'));
  assert.ok(companionSprite.includes('companion-sprite-step'));
  assert.ok(companionSprite.includes('will-change: transform'));
  assert.ok(companionSprite.includes('this keyframe must stay unscoped'));
  assert.ok(companionSprite.indexOf('@keyframes companion-sprite-step') > companionSprite.indexOf('</style>'));
  assert.ok(!companionSprite.includes('window.setInterval'));
});

test('normal chat debug avoids full-history work on streaming hot paths', () => {
  const runtimeState = readSource('src/stores/chatRuntimeState.ts');
  const renderableMessages = readSource('src/views/messenger/controller/messengerControllerRenderableMessages.ts');
  const watcher = readSource('src/stores/chatWatcher.ts');
  const sendActions = readSource('src/stores/chatSendActions.ts');
  const stopResume = readSource('src/stores/chatStopResumeActions.ts');
  const sessionOpen = readSource('src/stores/chatSessionOpenLoadActions.ts');

  assert.ok(runtimeState.includes('if (!isChatDebugEnabled() || !isChatDebugVerboseEnabled()) return null;'));
  assert.ok(runtimeState.includes('isChatDebugVerboseEnabled()'));
  assert.ok(renderableMessages.includes('isChatDebugVerboseEnabled()'));
  assert.ok(watcher.includes('isChatDebugVerboseEnabled()'));
  assert.ok(sendActions.includes('isChatDebugVerboseEnabled()'));
  assert.ok(stopResume.includes('isChatDebugVerboseEnabled()'));
  assert.ok(sessionOpen.includes('isChatDebugVerboseEnabled()'));

  const shadowStart = runtimeState.indexOf('export const inspectChatRuntimeShadow =');
  assert.ok(shadowStart >= 0);
  const shadowEnd = runtimeState.indexOf('export const applyCanonicalStreamRuntimeEvent =', shadowStart);
  assert.ok(shadowEnd > shadowStart);
  const shadowSource = runtimeState.slice(shadowStart, shadowEnd);
  requireInOrder(shadowSource, [
    'if (!isChatDebugEnabled() || !isChatDebugVerboseEnabled()) return null;',
    'const report = compareChatRuntimeShadow({',
    "chatDebugLog('chat.runtime.shadow', 'projection-legacy-drift'"
  ]);

  const renderLogStart = renderableMessages.indexOf('const logAgentRenderSource =');
  assert.ok(renderLogStart >= 0);
  const renderLogEnd = renderableMessages.indexOf('ctx.agentRenderableMessages = computed', renderLogStart);
  assert.ok(renderLogEnd > renderLogStart);
  const renderLogSource = renderableMessages.slice(renderLogStart, renderLogEnd);
  assert.ok(renderLogSource.includes('...(isChatDebugVerboseEnabled()'));
  assert.ok(renderLogSource.includes('messages: buildMessageIdentityDebugList('));
});

test('chat composer debounces draft persistence during typing', () => {
  const chatComposer = readSource('src/components/chat/ChatComposer.vue');

  assert.ok(chatComposer.includes('const DRAFT_PERSIST_DEBOUNCE_MS = 240;'));
  assert.ok(chatComposer.includes('let draftPersistTimer: ReturnType<typeof setTimeout> | null = null;'));
  assert.ok(chatComposer.includes('const schedulePersistDraftState = () => {'));
  assert.ok(chatComposer.includes('flushPersistDraftState(String(previousValue || \'\'));'));
  assert.ok(chatComposer.includes('flushPersistDraftState();\n  stopWorldComposerResize();'));

  const inputWatchStart = chatComposer.indexOf('watch(\n  () => inputText.value');
  assert.ok(inputWatchStart >= 0);
  const inputWatchEnd = chatComposer.indexOf('watch(\n  () => attachments.value', inputWatchStart);
  assert.ok(inputWatchEnd > inputWatchStart);
  const inputWatchSource = chatComposer.slice(inputWatchStart, inputWatchEnd);
  assert.ok(inputWatchSource.includes('schedulePersistDraftState();'));
  assert.ok(!inputWatchSource.includes('persistDraftState();'));
});

test('idle session detail hydration keeps the transcript lightweight while workflow metadata hydrates separately', () => {
  const sessionOpen = readSource('src/stores/chatSessionOpenLoadActions.ts');
  const cacheActions = readSource('src/stores/chatCacheActions.ts');
  const runtimeState = readSource('src/stores/chatRuntimeState.ts');
  const styles = readSource('src/styles/messenger.css');

  assert.ok(runtimeState.includes('export const shouldApplySessionEventsSnapshotToProjection ='));
  assert.ok(runtimeState.includes('payload.running === true'));
  assert.ok(runtimeState.includes('hasRuntimeControllers(runtime)'));
  assert.ok(sessionOpen.includes('shouldApplySessionEventsSnapshotToProjection(eventsPayload, runtime)'));
  assert.ok(sessionOpen.includes("events-snapshot-skip-idle-transcript"));
  assert.ok(cacheActions.includes('shouldApplySessionEventsSnapshotToProjection(eventsPayload, runtime)'));
  assert.ok(cacheActions.includes("events-snapshot-skip-idle-transcript"));
  assert.ok(sessionOpen.includes('void this.hydrateSessionWorkflowHistory(targetSessionId, this.messages);'));
  assert.ok(cacheActions.includes('loadSessionWorkflowEventsSnapshot'));
  assert.ok(cacheActions.includes('phase: \'history-workflow\''));

  assert.ok(!styles.includes('.messenger-message-main:hover .messenger-message-footer-copy'));
  assert.ok(!styles.includes('.messenger-message-bubble:hover .messenger-bubble-copy-btn'));
  assert.ok(!styles.includes('filter: saturate(1.06)'));
  assert.ok(styles.includes('.messenger-bubble-copy-btn:hover,\n.messenger-bubble-copy-btn:focus-visible'));
  assert.ok(styles.includes('.messenger-message-footer-copy:hover,\n.messenger-message-footer-copy:focus-visible'));
});

test('store visibleMessages getter materializes projection without legacy raw fallback', () => {
  const chatStore = readSource('src/stores/chat.ts');
  const watcher = readSource('src/stores/chatWatcher.ts');
  const renderAdapter = readSource('src/realtime/chat/chatRuntimeRenderAdapter.ts');
  const sessionOpenLoadActions = readSource('src/stores/chatSessionOpenLoadActions.ts');
  const messageCommands = readSource('src/views/messenger/controller/messengerControllerAgentMessageCommands.ts');
  const sendActions = readSource('src/stores/chatSendActions.ts');

  assert.ok(chatStore.includes('return materializeChatRuntimeMessages(state.runtimeProjection, sessionId || state.activeSessionId);'));
  assert.ok(sessionOpenLoadActions.includes('applyLocalChatMessageRuntimeEvent(this, {'));
  assert.ok(!messageCommands.includes("ctx.t('chat.command.newSuccess')"));
  assert.ok(sendActions.includes('return maxExplicitRound;'));
  assert.ok(!sendActions.includes('Math.max(maxExplicitRound, session.userTurns.length)'));
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

test('cached session message readers use projection materialization for secondary views', () => {
  const cacheActions = readSource('src/stores/chatCacheActions.ts');
  const beeroomRuntime = readSource('src/components/beeroom/useBeeroomMissionCanvasRuntime.ts');
  const messengerView = readSource('src/views/MessengerView.vue');
  const beeroomWorkbench = readSource('src/components/beeroom/BeeroomWorkbench.vue');
  const beeroomCanvas = readSource('src/components/beeroom/BeeroomMissionCanvas.vue');
  const swarmCanvasModel = readSource('src/components/beeroom/canvas/swarmCanvasModel.ts');
  const sharedHelpers = readSource('src/views/messenger/controller/messengerControllerSharedHelpers.ts');
  const runtimeToolLists = readSource('src/views/messenger/controller/messengerControllerRuntimeToolLists.ts');

  const cacheHelperStart = cacheActions.indexOf('getCachedSessionMessages(sessionId)');
  assert.ok(cacheHelperStart >= 0);
  const cacheHelperEnd = cacheActions.indexOf('getCachedSessions(agentId)', cacheHelperStart);
  assert.ok(cacheHelperEnd > cacheHelperStart);
  const cacheHelperSource = cacheActions.slice(cacheHelperStart, cacheHelperEnd);

  assert.ok(cacheActions.includes("from '@/realtime/chat/chatRuntimeRenderAdapter';"));
  assert.ok(cacheHelperSource.includes('const _projectionVersion = this.runtimeProjectionVersion;'));
  assert.ok(cacheHelperSource.includes('materializeChatRuntimeMessages(this.runtimeProjection, targetId);'));
  assert.ok(cacheHelperSource.includes('if (projected.length > 0)'));
  assert.ok(cacheHelperSource.includes('const cachedMessages = Array.isArray(cached) ? cached : [];'));
  assert.ok(cacheHelperSource.includes('activeSessionId === targetId'));
  assert.ok(cacheHelperSource.includes('Array.isArray(this.messages) && this.messages.length > 0'));
  assert.ok(cacheHelperSource.includes('const resolveMessageTime = (message: Record<string, unknown> | null | undefined) =>'));
  assert.ok(cacheHelperSource.includes('cachedLatest > activeLatest'));
  assert.ok(cacheHelperSource.includes('getSessionMessages(targetId)'));
  assert.ok(!cacheHelperSource.includes('getSessionMessages(sessionId)'));

  const readDispatchStart = beeroomRuntime.indexOf('const readDispatchSessionMessages = (sessionId: string)');
  assert.ok(readDispatchStart >= 0);
  const readDispatchEnd = beeroomRuntime.indexOf('const nextManualMessageKey', readDispatchStart);
  assert.ok(readDispatchEnd > readDispatchStart);
  const readDispatchSource = beeroomRuntime.slice(readDispatchStart, readDispatchEnd);

  assert.ok(readDispatchSource.includes('chatStore.getCachedSessionMessages(targetId)'));
  assert.ok(!readDispatchSource.includes('chatStore.messages'));

  const mapDispatchStart = beeroomRuntime.indexOf('const mapSessionChatMessage = (');
  assert.ok(mapDispatchStart >= 0);
  const mapDispatchEnd = beeroomRuntime.indexOf('const readDispatchSessionMessages = (sessionId: string)', mapDispatchStart);
  assert.ok(mapDispatchEnd > mapDispatchStart);
  const mapDispatchSource = beeroomRuntime.slice(mapDispatchStart, mapDispatchEnd);
  assert.ok(mapDispatchSource.includes('if (!body) return null;'));
  assert.ok(!mapDispatchSource.includes('const assistantStillRunning ='));
  assert.ok(!mapDispatchSource.includes("if (role === 'assistant' && !historyId"));
  assert.ok(!mapDispatchSource.includes('streamEventId > 0 ||'));

  assert.ok(beeroomRuntime.includes('function resolveDispatchProjectionWorkflowItems(sessionId: unknown): BeeroomWorkflowItem[]'));
  assert.ok(beeroomRuntime.includes('function resolveDispatchProjectionMessageUserTurnId(message: Record<string, unknown> | null): string'));
  assert.ok(beeroomRuntime.includes('latestAssistantTurnId'));
  assert.ok(beeroomRuntime.includes('const _projectionVersion = Number(chatStore.runtimeProjectionVersion || 0);'));
  assert.ok(beeroomRuntime.includes('chatStore.getCachedSessionMessages(targetId)'));
  assert.ok(beeroomRuntime.includes('Array.isArray(message.workflowItems)'));
  assert.ok(beeroomRuntime.includes('Array.isArray(message.workflow_items)'));
  assert.ok(beeroomRuntime.includes('workflowItems'));
  assert.ok(beeroomRuntime.includes('...overlaid,'));
  assert.ok(beeroomRuntime.includes('applyCanonicalClientMessageSubmittedRuntimeEvent(chatStore, {'));
  assert.ok(beeroomRuntime.includes('applyCanonicalStreamRuntimeEvent('));
  assert.ok(beeroomRuntime.includes("phase: 'beeroom-dispatch'"));
  assert.ok(beeroomRuntime.includes('cancelOnAbort: false'));
  assert.ok(beeroomRuntime.includes('const detachedLocally = error?.name === \'AbortError\' && !dispatchStopRequested;'));
  assert.ok(beeroomRuntime.includes('keepSending: preserveLiveDispatch'));
  assert.ok(messengerView.includes(':active-chat-session-id="beeroomActiveChatSessionId"'));
  assert.ok(messengerView.includes(':active-chat-agent-id="beeroomActiveChatAgentId"'));
  assert.ok(messengerView.includes('lastMessageSectionSessionId'));
  assert.ok(messengerView.includes("section !== 'messages' || !sessionId"));
  assert.ok(beeroomWorkbench.includes(':active-chat-session-id="activeChatSessionId"'));
  assert.ok(beeroomWorkbench.includes(':active-chat-agent-id="activeChatAgentId"'));
  assert.ok(beeroomCanvas.includes('activeChatSessionId?: string;'));
  assert.ok(beeroomCanvas.includes('activeChatAgentId?: string;'));
  assert.ok(beeroomCanvas.includes('fixedMotherDispatchSessionId: activeChatSessionIdRef'));
  assert.ok(beeroomCanvas.includes('fixedMotherDispatchAgentId: activeChatAgentIdRef'));
  assert.ok(beeroomRuntime.includes('fixedMotherDispatchAgentId?: Ref<unknown>;'));
  assert.ok(beeroomRuntime.includes('const fixedMotherDispatchAgentId = computed(() =>'));
  assert.ok(beeroomRuntime.includes('const explicitFixedAgentId = String(fixedMotherDispatchAgentId.value || \'\').trim();'));
  assert.ok(beeroomRuntime.includes('const currentDispatchAgentId ='));
  assert.ok(beeroomRuntime.includes('explicitFixedAgentId ||'));
  assert.ok(beeroomRuntime.includes('const next = loadOptions.forceReplace === true'));
  assert.ok(beeroomRuntime.includes('forceReplace: Boolean(fixedMotherDispatchSessionId.value)'));
  assert.ok(beeroomRuntime.includes('if (!isTool) {'));
  assert.ok(swarmCanvasModel.includes('workflowItems?: BeeroomWorkflowItem[];'));
  assert.ok(swarmCanvasModel.includes('const filterToolWorkflowItems = (items: unknown): BeeroomWorkflowItem[]'));
  assert.ok(swarmCanvasModel.includes('filterToolWorkflowItems(preview.workflowItems)'));
  assert.ok(swarmCanvasModel.includes('{ includeEventFallback: false }'));
  assert.ok(!swarmCanvasModel.includes('includeEventFallback: true'));
  assert.ok(!swarmCanvasModel.includes('return workflowLines.length > 0 ? workflowLines : buildSubagentSummaryLines'));
  assert.ok(swarmCanvasModel.includes('const dispatchWorkflowLines = buildDispatchPreviewLines(runtimeDispatch, statusLabel, options.t);'));
  assert.ok(swarmCanvasModel.includes('runtimeTargetNode.workflowLines = dispatchWorkflowLines;'));
  assert.ok(readSource('src/components/beeroom/useBeeroomDispatchSessionPreview.ts').includes('workflowItems = buildSessionWorkflowItems('));

  assert.ok(beeroomRuntime.includes('Number(chatStore.runtimeProjectionVersion || 0)'));
  assert.ok(beeroomRuntime.includes("scheduleDispatchMessageRefresh('runtime-projection', {"));
  assert.ok(beeroomRuntime.includes('hydrate: false,'));
  assert.ok(runtimeToolLists.includes('ctx.resolveEffectiveSessionBusy(sessionId)'));
  assert.ok(!runtimeToolLists.includes('!sessionId || !ctx.isSessionBusy(sessionId)'));
  assert.ok(beeroomRuntime.includes('emitAgentRuntimeRefresh({'));

  const activityStart = sharedHelpers.indexOf('ctx.resolveSessionActivityTimestamp = function resolveSessionActivityTimestamp');
  assert.ok(activityStart >= 0);
  const activityEnd = sharedHelpers.indexOf('ctx.resolveSessionRecordById', activityStart);
  assert.ok(activityEnd > activityStart);
  const activitySource = sharedHelpers.slice(activityStart, activityEnd);
  assert.ok(activitySource.includes('ctx.chatStore.getCachedSessionMessages(sessionId)'));
  assert.ok(activitySource.includes('ctx.resolveLatestConversationMessageTimestamp'));
  assert.ok(activitySource.includes('return Math.max(fieldTimestamp, messageTimestamp);'));
});
