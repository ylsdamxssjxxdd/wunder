import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

test('messenger message stats timer only keys off the latest visible assistant', () => {
  const source = readFileSync(
    resolve(process.cwd(), 'src/views/messenger/controller/messengerControllerRenderableMessages.ts'),
    'utf8'
  );
  const hasLiveAssistantStatsIndex = source.indexOf('ctx.hasLiveAssistantStats = computed(() => {');
  assert.ok(hasLiveAssistantStatsIndex >= 0);
  const stopTimerIndex = source.indexOf('const stopMessageStatsTimer = () => {', hasLiveAssistantStatsIndex);
  assert.ok(stopTimerIndex > hasLiveAssistantStatsIndex);
  const body = source.slice(hasLiveAssistantStatsIndex, stopTimerIndex);

  assert.ok(body.includes('const latestVisibleAssistant = ctx.latestVisibleAgentAssistantMessage.value'));
  assert.ok(body.includes('isAssistantMessageRunning(latestVisibleAssistant)'));
  assert.ok(body.includes('hasAssistantWaitingForCurrentOutput(latestVisibleAssistant)'));
  assert.ok(body.includes('hasActiveSubagentItems(latestVisibleAssistant?.subagents)'));
  assert.ok(!body.includes('ctx.agentRenderableMessages.value.some('));
});

test('desktop safe mode is exposed through runtime config and does not depend on launch flags alone', () => {
  const desktopConfig = readFileSync(resolve(process.cwd(), 'src/config/desktop.ts'), 'utf8');
  const tauriBridge = readFileSync(resolve(process.cwd(), '..', 'desktop', 'tauri', 'bridge.rs'), 'utf8');
  assert.ok(desktopConfig.includes('safe_mode?: boolean;'));
  assert.ok(desktopConfig.includes('safe_mode: asBoolean(source.safe_mode)'));
  assert.ok(desktopConfig.includes('getDesktopRuntime()?.safe_mode'));
  assert.ok(tauriBridge.includes('pub safe_mode: bool'));
  assert.ok(tauriBridge.includes('let safe_mode = std::env::var("WUNDER_DESKTOP_SAFE_MODE")'));
  assert.ok(tauriBridge.includes("localStorage.setItem('wunder_desktop_safe_mode', '1');"));
});

test('desktop safe mode skips chat session bootstrap and route restore', () => {
  const routeBootstrap = readFileSync(
    resolve(process.cwd(), 'src/views/messenger/controller/messengerControllerLifecycleRouteBootstrap.ts'),
    'utf8'
  );
  assert.ok(routeBootstrap.includes('if (isDesktopSafeModeEnabled())'));
  assert.ok(routeBootstrap.includes('return Promise.resolve();'));
  assert.ok(routeBootstrap.includes("ctx.sessionHub.setSection('more');"));
  assert.ok(routeBootstrap.includes("ctx.settingsPanelMode.value = 'profile';"));
});

test('desktop chat avoids recursive detail hydration and watcher rebuild loops', () => {
  const recoverySource = readFileSync(
    resolve(process.cwd(), 'src/views/messenger/activeChatRealtimeRecovery.ts'),
    'utf8'
  );
  const realtimeRecoveryActions = readFileSync(resolve(process.cwd(), 'src/stores/chatRealtimeRecoveryActions.ts'), 'utf8');
  const sessionOpenLoadActions = readFileSync(resolve(process.cwd(), 'src/stores/chatSessionOpenLoadActions.ts'), 'utf8');
  const watcherSource = readFileSync(resolve(process.cwd(), 'src/stores/chatWatcher.ts'), 'utf8');

  assert.ok(recoverySource.includes("active-realtime-recovery-disabled"));
  assert.ok(recoverySource.includes("desktopMode?.value"));
  assert.ok(realtimeRecoveryActions.includes('startWatcherAfterHydration: false'));
  assert.ok(sessionOpenLoadActions.includes('const sessionDetailLoadInFlight = new Map<string, Promise<unknown>>();'));
  assert.ok(sessionOpenLoadActions.includes('options.startWatcherAfterHydration === false ?'));
  assert.ok(sessionOpenLoadActions.includes('const allowStartWatcherAfterHydration = options.startWatcherAfterHydration !== false;'));
  assert.ok(watcherSource.includes('const desktopMode = isDesktopModeEnabled();'));
  assert.ok(watcherSource.includes('if (desktopMode) return;'));
  assert.ok(watcherSource.includes('pendingMessage || !desktopMode'));
  assert.ok(!watcherSource.includes('desktop-watcher-short-circuit'));
  assert.ok(!watcherSource.includes('preserveWatcher: false, forceHydrateForeground: true'));
});
