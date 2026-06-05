const {
  app,
  BrowserWindow,
  clipboard,
  dialog,
  Menu,
  Tray,
  nativeImage,
  ipcMain,
  desktopCapturer,
  screen,
  Notification,
  session,
  systemPreferences,
  shell
} = require('electron')
const { spawn, spawnSync } = require('child_process')
const fs = require('fs')
const net = require('net')
const Module = require('module')
const os = require('os')
const path = require('path')
const {
  DEFAULT_MINIMIZE_RESTORE_COOLDOWN_MS,
  createWindowVisibilityGuard
} = require('./windowVisibilityGuard')
const {
  resolveDesktopEffectWindowsDisabledReason,
  shouldDisableElectronHardwareAcceleration
} = require('./desktopCompatibility')

const resolveRuntimeModuleRoots = () => {
  const roots = []
  if (process.resourcesPath) {
    roots.push(path.join(process.resourcesPath, 'runtime-deps'))
    roots.push(path.join(process.resourcesPath, 'node_modules'))
  }
  roots.push(path.join(__dirname, '..', 'resources', 'runtime-deps'))
  roots.push(path.join(__dirname, '..', 'resources', 'node_modules'))
  roots.push(path.join(__dirname, '..', 'node_modules'))
  return roots
}

const registerRuntimeModuleRoots = () => {
  const existing = String(process.env.NODE_PATH || '')
    .split(path.delimiter)
    .map((item) => item.trim())
    .filter((item) => item)
  const availableRoots = resolveRuntimeModuleRoots().filter((root) => fs.existsSync(root))
  if (!availableRoots.length) {
    return
  }
  const merged = Array.from(new Set([...availableRoots, ...existing]))
  process.env.NODE_PATH = merged.join(path.delimiter)
  Module._initPaths()
}

const resolveUpdaterCandidates = () => {
  const candidates = []
  for (const root of resolveRuntimeModuleRoots()) {
    candidates.push(path.join(root, 'electron-updater'))
  }
  return candidates
}

const resolveUpdaterDisableMarker = () => {
  const candidates = []
  if (process.resourcesPath) {
    candidates.push(path.join(process.resourcesPath, 'disable-updater.flag'))
    candidates.push(path.join(process.resourcesPath, 'win7-disable-updater.flag'))
  }
  candidates.push(path.join(__dirname, '..', 'resources', 'disable-updater.flag'))
  candidates.push(path.join(__dirname, '..', 'resources', 'win7-disable-updater.flag'))
  for (const candidate of candidates) {
    if (candidate && fs.existsSync(candidate)) {
      return candidate
    }
  }
  return ''
}

const resolveDesktopSafeModeMarker = () => {
  const candidates = []
  if (process.resourcesPath) {
    candidates.push(path.join(process.resourcesPath, 'safe-mode.flag'))
    candidates.push(path.join(process.resourcesPath, 'win7-safe-mode.flag'))
  }
  candidates.push(path.join(__dirname, '..', 'resources', 'safe-mode.flag'))
  candidates.push(path.join(__dirname, '..', 'resources', 'win7-safe-mode.flag'))
  for (const candidate of candidates) {
    if (candidate && fs.existsSync(candidate)) {
      return candidate
    }
  }
  return ''
}

const detectWin7PackageFlavor = () => {
  if (process.platform !== 'win32') {
    return false
  }
  try {
    const userDataPath = String(app.getPath('userData') || '').toLowerCase()
    return userDataPath.includes('wunder-desktop-electron-win7')
  } catch {
    return false
  }
}

registerRuntimeModuleRoots()

const updaterDisableMarker = resolveUpdaterDisableMarker()
const desktopSafeModeMarker = resolveDesktopSafeModeMarker()
const runningInAppImage =
  process.platform === 'linux' && Boolean(String(process.env.APPIMAGE || '').trim())
const runningWin7PackageFlavor = detectWin7PackageFlavor()
const updaterDisabledReason = updaterDisableMarker
  ? `marker: ${updaterDisableMarker}`
  : runningInAppImage
    ? 'appimage'
    : runningWin7PackageFlavor
      ? 'win7-package'
      : ''
const updaterDisabledByBuild = Boolean(updaterDisabledReason)
const desktopSafeModeEnabled =
  Boolean(desktopSafeModeMarker) || String(process.env.WUNDER_DESKTOP_SAFE_MODE || '').trim() === '1'
if (desktopSafeModeEnabled) {
  process.env.WUNDER_DESKTOP_SAFE_MODE = '1'
  app.disableHardwareAcceleration()
  console.info(
    `[desktop-debug][electron] safe mode enabled by ${desktopSafeModeMarker || 'env'}`
  )
}
let autoUpdater = null
if (!updaterDisabledByBuild) {
  try {
    ;({ autoUpdater } = require('electron-updater'))
  } catch (error) {
    for (const candidate of resolveUpdaterCandidates()) {
      try {
        ;({ autoUpdater } = require(candidate))
        console.info(`[updater] loaded bundled updater module from: ${candidate}`)
        break
      } catch {
        // Continue probing fallback locations.
      }
    }
    if (!autoUpdater) {
      // Keep the desktop app bootable even if auto-update assets are missing.
      console.warn('[updater] electron-updater is unavailable, auto update disabled:', error)
    }
  }
} else {
  console.info(`[updater] disabled by ${updaterDisabledReason}`)
}

let mainWindow = null
let bridgeProcess = null
let bridgePort = null
let bridgeWebBase = null
let bridgeRestarting = false
let updateTask = null
let updaterReady = false
let tray = null
let closePromptInFlight = false
let closeBehavior = 'ask'
let linuxDesktopIntegrationScheduled = false
const parseEnvNonNegativeNumber = (raw, fallbackValue) => {
  const parsed = Number(raw)
  if (Number.isFinite(parsed) && parsed >= 0) {
    return parsed
  }
  return fallbackValue
}
const sanitizeRendererStagePayload = (payload) => {
  const source = payload && typeof payload === 'object' ? payload : {}
  const output = {}
  for (const [key, value] of Object.entries(source)) {
    const normalizedKey = String(key || '').trim().slice(0, 48)
    if (!normalizedKey) {
      continue
    }
    if (typeof value === 'number' || typeof value === 'boolean') {
      output[normalizedKey] = value
      continue
    }
    if (value === null) {
      output[normalizedKey] = null
      continue
    }
    output[normalizedKey] = String(value ?? '').slice(0, 160)
  }
  return output
}
const resolveRendererCrashStatePath = () =>
  path.join(app.getPath('userData'), 'desktop-renderer-crash-state.json')
const readRendererCrashState = () => {
  try {
    const statePath = resolveRendererCrashStatePath()
    if (!fs.existsSync(statePath)) {
      return {}
    }
    const parsed = JSON.parse(fs.readFileSync(statePath, 'utf8'))
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}
const writeRendererCrashState = (patch = {}) => {
  try {
    const statePath = resolveRendererCrashStatePath()
    const current = readRendererCrashState()
    const next = {
      ...current,
      ...patch,
      updatedAt: Date.now()
    }
    fs.mkdirSync(path.dirname(statePath), { recursive: true })
    const tempPath = `${statePath}.${process.pid}.${Date.now()}.tmp`
    fs.writeFileSync(tempPath, `${JSON.stringify(next, null, 2)}\n`, 'utf8')
    fs.renameSync(tempPath, statePath)
  } catch {
    // Ignore renderer crash-state persistence failures.
  }
}
const desktopRendererCrashState = readRendererCrashState()
const rendererCompatibilityModeEnabled =
  String(process.env.WUNDER_DESKTOP_RENDERER_COMPAT_MODE || '').trim() === '1' ||
  desktopRendererCrashState.compatibleGraphics === true ||
  runningWin7PackageFlavor
if (rendererCompatibilityModeEnabled) {
  process.env.WUNDER_DISABLE_GPU = '1'
  console.info('[desktop-debug][electron] renderer compatibility mode enabled')
}
const disableBackgroundThrottling = process.env.WUNDER_DISABLE_BACKGROUND_THROTTLING === '1'
const suppressGpuWarnings = process.env.WUNDER_SUPPRESS_GPU_WARNINGS !== '0'
const bridgeVerboseLogs = process.env.WUNDER_BRIDGE_LOG_VERBOSE !== '0'
const desktopEffectWindowsDisabledReason = resolveDesktopEffectWindowsDisabledReason({
  env: process.env,
  platform: process.platform,
  release: os.release()
})
const desktopEffectWindowsEnabled = !desktopEffectWindowsDisabledReason
const disableElectronHardwareAcceleration = shouldDisableElectronHardwareAcceleration({
  env: process.env,
  platform: process.platform,
  release: os.release()
})
if (!desktopEffectWindowsEnabled) {
  console.info(`[desktop-effects] native overlay windows disabled by ${desktopEffectWindowsDisabledReason}`)
}
const defaultLoadingShellDelayMs = app.isPackaged ? 1200 : 220
const loadingShellDelayMs = parseEnvNonNegativeNumber(
  process.env.WUNDER_LOADING_SHELL_DELAY_MS,
  defaultLoadingShellDelayMs
)
const mainWindowMinimizeRestoreCooldownMs = parseEnvNonNegativeNumber(
  process.env.WUNDER_MAIN_WINDOW_MINIMIZE_GUARD_MS,
  DEFAULT_MINIMIZE_RESTORE_COOLDOWN_MS
)
const mainWindowVisibilityGuard = createWindowVisibilityGuard({
  minimizeRestoreCooldownMs: mainWindowMinimizeRestoreCooldownMs
})

const SCREENSHOT_HIDE_DELAY_MS = 220
const SCREENSHOT_SELECTOR_RESULT_CHANNEL = 'wunder:screenshot-region-selected'
const SCREENSHOT_SELECTOR_CANCEL_CHANNEL = 'wunder:screenshot-region-canceled'
const OVERLAY_UPDATE_CHANNEL = 'wunder:overlay-update'
const OVERLAY_HIDE_CHANNEL = 'wunder:overlay-hide'

const DEFAULT_OVERLAY_HINT_MS = 2000
const DEFAULT_OVERLAY_DONE_MS = 2000
const DEFAULT_OVERLAY_MIN_HIDE_MS = 400
const OVERLAY_BOX_SIZE = 80

const startupTimingEnabled =
  process.env.WUNDER_STARTUP_TIMING !== undefined
    ? process.env.WUNDER_STARTUP_TIMING !== '0'
    : true
const startupBootNs = process.hrtime.bigint()

const elapsedMsSince = (startedNs) => Number(process.hrtime.bigint() - startedNs) / 1_000_000

const normalizeStartupField = (value) => {
  if (value === null || value === undefined) {
    return ''
  }
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) {
      return ''
    }
    return String(Math.round(value * 10) / 10)
  }
  return String(value).trim().replace(/\s+/g, '_')
}

const startupFieldsToText = (fields) =>
  Object.entries(fields || {})
    .map(([key, value]) => {
      const normalized = normalizeStartupField(value)
      if (!normalized) {
        return ''
      }
      return `${key}=${normalized}`
    })
    .filter((item) => item)
    .join(' ')

const logStartupSegment = (scope, segment, startedNs, fields = {}) => {
  if (!startupTimingEnabled) {
    return
  }
  const elapsedMs = elapsedMsSince(startedNs)
  const totalMs = elapsedMsSince(startupBootNs)
  const extras = startupFieldsToText(fields)
  console.info(
    `[startup][${scope}] segment=${segment} elapsed_ms=${elapsedMs.toFixed(1)} total_ms=${totalMs.toFixed(1)}${extras ? ` ${extras}` : ''}`
  )
}

const logStartupPoint = (scope, point, fields = {}) => {
  if (!startupTimingEnabled) {
    return
  }
  const totalMs = elapsedMsSince(startupBootNs)
  const extras = startupFieldsToText(fields)
  console.info(
    `[startup][${scope}] point=${point} total_ms=${totalMs.toFixed(1)}${extras ? ` ${extras}` : ''}`
  )
}

logStartupPoint('electron', 'main_process_loaded', {
  pid: process.pid,
  packaged: app.isPackaged ? 1 : 0
})

const createUpdateSnapshot = () => ({
  phase: 'idle',
  currentVersion: app.getVersion(),
  latestVersion: '',
  downloaded: false,
  progress: 0,
  message: ''
})

let updateState = createUpdateSnapshot()

let overlayWindow = null
let overlayHideTimer = null
let companionWindow = null
let companionWindowMouseIgnored = false
let companionWindowShapeEnabled = false
let companionTransientState = ''
let companionTransientTimer = null
let companionTransientUntil = 0
let mainWindowSendReady = false
const pendingMainWindowMessages = []
let companionState = {
  enabled: false,
  key: '',
  agentId: '',
  selectedId: '',
  displayName: '',
  description: '',
  spritesheetDataUrl: '',
  state: 'idle',
  scale: 1,
  x: 28,
  y: 28,
  message: '',
  messageKind: 'info',
  messageVisible: false
}
const companionRuntimes = new Map()
const COMPANION_COMMAND_CHANNEL = 'wunder:companion-command'
const COMPANION_FRAME_WIDTH = 192
const COMPANION_FRAME_HEIGHT = 208
const COMPANION_SCREEN_MARGIN = 8
const COMPANION_MIN_SCALE = 0.5
const COMPANION_MAX_SCALE = 1.6
const COMPANION_BASE_FALLBACK_STATE = 'idle'
const MAIN_WINDOW_MESSAGE_RETRY_MS = 120
const MAIN_WINDOW_MESSAGE_MAX_RETRIES = 6
const COMPANION_NON_PERSISTENT_STATES = new Set([
  'running-left',
  'running-right',
  'waving'
])

const normalizeCompanionBaseState = (state) => {
  const normalized = String(state || '').trim().toLowerCase()
  if (!normalized || COMPANION_NON_PERSISTENT_STATES.has(normalized)) {
    return COMPANION_BASE_FALLBACK_STATE
  }
  return normalized
}

const flushMainWindowMessages = () => {
  if (!mainWindow || mainWindow.isDestroyed() || mainWindow.webContents.isDestroyed()) {
    return false
  }
  if (!mainWindowSendReady || mainWindow.webContents.isLoadingMainFrame()) {
    return false
  }
  while (pendingMainWindowMessages.length) {
    const item = pendingMainWindowMessages.shift()
    try {
      mainWindow.webContents.send(item.channel, item.payload)
    } catch (error) {
      console.warn(`[desktop-ipc] delayed send failed on ${item.channel}:`, error)
      return false
    }
  }
  return true
}

const sendMainWindowMessage = (channel, payload, options = {}) => {
  const retry = Math.max(0, Number(options.retry || 0))
  if (!mainWindow || mainWindow.isDestroyed() || mainWindow.webContents.isDestroyed()) {
    return false
  }
  if (!mainWindowSendReady || mainWindow.webContents.isLoadingMainFrame()) {
    if (retry <= 0) {
      pendingMainWindowMessages.push({ channel, payload })
    }
    return false
  }
  try {
    mainWindow.webContents.send(channel, payload)
    return true
  } catch (error) {
    if (retry >= MAIN_WINDOW_MESSAGE_MAX_RETRIES) {
      console.warn(`[desktop-ipc] send failed on ${channel}:`, error)
      return false
    }
    setTimeout(() => {
      sendMainWindowMessage(channel, payload, { retry: retry + 1 })
    }, MAIN_WINDOW_MESSAGE_RETRY_MS)
    return false
  }
}

const normalizeCompanionWindowKey = (value = {}) => {
  const source = value && typeof value === 'object' ? value : {}
  const key = String(
    source.key ||
    source.agentId ||
    source.agent_id ||
    source.selectedId ||
    source.selected_id ||
    source.id ||
    ''
  ).trim()
  return key || '__default__'
}

const normalizeCompanionKeepKeys = (value) => {
  const keys = new Set()
  if (!Array.isArray(value)) {
    return keys
  }
  value.forEach((item) => {
    const key = String(item || '').trim()
    if (key) {
      keys.add(key)
    }
  })
  return keys
}

const getCompanionRuntime = (value = {}) => {
  const key = normalizeCompanionWindowKey(value)
  let runtime = companionRuntimes.get(key)
  if (!runtime) {
    runtime = {
      key,
      window: null,
      mouseIgnored: false,
      shapeEnabled: false,
      transientState: '',
      transientTimer: null,
      transientUntil: 0,
      state: {
        ...companionState,
        key
      }
    }
    companionRuntimes.set(key, runtime)
  }
  return runtime
}

const findCompanionRuntime = (value = {}) => {
  const source = value && typeof value === 'object' ? value : {}
  const key = normalizeCompanionWindowKey(source)
  if (companionRuntimes.has(key)) {
    return companionRuntimes.get(key)
  }
  const agentId = String(source.agentId || source.agent_id || '').trim()
  if (agentId) {
    const matched = Array.from(companionRuntimes.values()).find((runtime) => runtime.state.agentId === agentId)
    if (matched) {
      return matched
    }
  }
  const selectedId = String(source.selectedId || source.selected_id || source.id || '').trim()
  if (selectedId) {
    const matched = Array.from(companionRuntimes.values()).find((runtime) => runtime.state.selectedId === selectedId)
    if (matched) {
      return matched
    }
  }
  return companionRuntimes.values().next().value || null
}

const rememberPrimaryCompanionRuntime = (runtime) => {
  if (!runtime) {
    return
  }
  companionState = runtime.state
  companionWindow = runtime.window
  companionWindowMouseIgnored = runtime.mouseIgnored
  companionWindowShapeEnabled = runtime.shapeEnabled
}

const clearRuntimeTransientState = (runtime) => {
  if (!runtime) {
    return
  }
  if (runtime.transientTimer) {
    clearTimeout(runtime.transientTimer)
    runtime.transientTimer = null
  }
  runtime.transientState = ''
  runtime.transientUntil = 0
}

const clearCompanionTransientState = (target = null) => {
  if (target) {
    clearRuntimeTransientState(target.state ? target : findCompanionRuntime(target))
    return
  }
  Array.from(companionRuntimes.values()).forEach((runtime) => clearRuntimeTransientState(runtime))
  if (companionTransientTimer) {
    clearTimeout(companionTransientTimer)
    companionTransientTimer = null
  }
  companionTransientState = ''
  companionTransientUntil = 0
}

const setCompanionTransientState = (target, state, durationMs) => {
  const runtime = typeof target === 'string'
    ? findCompanionRuntime(companionState) || getCompanionRuntime(companionState)
    : getCompanionRuntime(target || companionState)
  const rawState = typeof target === 'string' ? target : state
  const rawDurationMs = typeof target === 'string' ? state : durationMs
  const nextState = String(rawState || '').trim().toLowerCase()
  if (!nextState) {
    clearRuntimeTransientState(runtime)
    return
  }
  clearRuntimeTransientState(runtime)
  const safeDurationMs = Math.max(120, Number(rawDurationMs || 0))
  runtime.transientState = nextState
  runtime.transientUntil = Date.now() + safeDurationMs
  runtime.transientTimer = setTimeout(() => {
    runtime.transientTimer = null
    runtime.transientState = ''
    runtime.transientUntil = 0
    if (runtime.window && !runtime.window.isDestroyed()) {
      renderCompanionWindow(runtime)
    }
  }, safeDurationMs)
}

const resolveRenderedCompanionState = (runtime = null) => {
  const target = runtime || findCompanionRuntime(companionState)
  if (target?.transientState && target.transientUntil > Date.now()) {
    return target.transientState
  }
  return normalizeCompanionBaseState((target?.state || companionState).state)
}

const createOverlayHtml = () => `<!doctype html>
<html>
<head>
<meta charset="utf-8" />
<style>
  html, body {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    background: transparent;
    overflow: hidden;
    pointer-events: none;
    font-family: "Segoe UI", "Helvetica Neue", Arial, sans-serif;
  }
  #overlay-root {
    position: relative;
    width: 100%;
    height: 100%;
  }
  #controller-hint {
    position: absolute;
    left: 0;
    top: 0;
    pointer-events: none;
  }
  #controller-box {
    position: absolute;
    width: ${OVERLAY_BOX_SIZE}px;
    height: ${OVERLAY_BOX_SIZE}px;
    border-radius: 6px;
    border: 3px solid rgba(255, 60, 60, 0.9);
    background: rgba(255, 60, 60, 0.18);
    box-sizing: border-box;
  }
  #controller-box.done {
    border-color: rgba(60, 220, 120, 0.9);
    background: rgba(60, 220, 120, 0.18);
  }
  #controller-cross {
    position: absolute;
    left: 50%;
    top: 50%;
    width: 24px;
    height: 24px;
    transform: translate(-50%, -50%);
    pointer-events: none;
  }
  #controller-cross::before,
  #controller-cross::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 50%;
    background: rgba(255, 80, 80, 0.9);
    transform: translate(-50%, -50%);
  }
  #controller-box.done #controller-cross::before,
  #controller-box.done #controller-cross::after {
    background: rgba(80, 255, 160, 0.9);
  }
  #controller-cross::before {
    width: 20px;
    height: 2px;
  }
  #controller-cross::after {
    width: 2px;
    height: 20px;
  }
  #controller-label {
    position: absolute;
    padding: 6px 10px;
    border-radius: 10px;
    font-size: 18px;
    font-weight: 600;
    color: rgba(255, 235, 235, 0.95);
    background: rgba(20, 20, 20, 0.6);
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.6);
    max-width: 420px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  #controller-box.done + #controller-label {
    color: rgba(230, 255, 240, 0.95);
  }
  #monitor-countdown {
    position: absolute;
    left: 50%;
    top: 18px;
    transform: translateX(-50%);
    font-size: 26px;
    font-weight: 700;
    color: rgba(70, 160, 255, 0.95);
    text-shadow: 0 2px 6px rgba(0, 0, 0, 0.5);
    padding: 6px 12px;
    border-radius: 10px;
    background: rgba(10, 14, 20, 0.4);
    white-space: nowrap;
  }
  .hidden {
    display: none;
  }
</style>
</head>
<body>
  <div id="overlay-root">
    <div id="controller-hint" class="hidden">
      <div id="controller-box">
        <div id="controller-cross"></div>
      </div>
      <div id="controller-label"></div>
    </div>
    <div id="monitor-countdown" class="hidden"></div>
  </div>
  <script>
    const { ipcRenderer } = require('electron');
    const hint = document.getElementById('controller-hint');
    const box = document.getElementById('controller-box');
    const label = document.getElementById('controller-label');
    const monitor = document.getElementById('monitor-countdown');
    let monitorTimer = null;
    let monitorDeadline = 0;
    let monitorTotal = 0;

    const clearMonitor = () => {
      if (monitorTimer) {
        clearInterval(monitorTimer);
        monitorTimer = null;
      }
      monitorDeadline = 0;
      monitorTotal = 0;
      monitor.textContent = '';
      monitor.classList.add('hidden');
    };

    const clearController = () => {
      hint.classList.add('hidden');
    };

    const updateMonitorText = () => {
      if (!monitorDeadline) return;
      const now = Date.now();
      const remainMs = Math.max(0, monitorDeadline - now);
      const remainSec = remainMs <= 0 ? 0 : Math.ceil(remainMs / 1000);
      if (monitorTotal <= 0 || remainSec <= 0) {
        monitor.textContent = 'Capturing soon...';
      } else {
        monitor.textContent = 'Capturing in ' + remainSec + 's';
      }
    };

    const showController = (payload) => {
      const x = Number(payload?.x || 0);
      const y = Number(payload?.y || 0);
      if (!Number.isFinite(x) || !Number.isFinite(y)) return;
      const desc = String(payload?.description || 'controller').trim() || 'controller';
      const state = String(payload?.state || '').toLowerCase();
      const half = ${OVERLAY_BOX_SIZE} / 2;

      box.classList.toggle('done', state === 'done');
      label.textContent = desc;
      hint.classList.remove('hidden');

      const left = Math.round(x - half);
      const top = Math.round(y - half);
      box.style.left = left + 'px';
      box.style.top = top + 'px';

      requestAnimationFrame(() => {
        const labelRect = label.getBoundingClientRect();
        let labelLeft = Math.round(x - labelRect.width / 2);
        labelLeft = Math.max(8, Math.min(labelLeft, window.innerWidth - labelRect.width - 8));
        let labelTop = top - labelRect.height - 10;
        if (labelTop < 6) {
          labelTop = top + ${OVERLAY_BOX_SIZE} + 10;
        }
        label.style.left = labelLeft + 'px';
        label.style.top = labelTop + 'px';
      });
    };

    const showMonitor = (payload) => {
      const waitMs = Math.max(0, Number(payload?.waitMs ?? payload?.wait_ms ?? 0));
      monitorTotal = waitMs;
      monitorDeadline = Date.now() + waitMs;
      monitor.classList.remove('hidden');
      updateMonitorText();
      if (!monitorTimer) {
        monitorTimer = setInterval(updateMonitorText, 120);
      }
    };

    ipcRenderer.on('${OVERLAY_UPDATE_CHANNEL}', (event, payload) => {
      if (!payload || typeof payload !== 'object') return;
      if (payload.mode === 'monitor') {
        clearController();
        showMonitor(payload);
        return;
      }
      clearMonitor();
      showController(payload);
    });

    ipcRenderer.on('${OVERLAY_HIDE_CHANNEL}', () => {
      clearController();
      clearMonitor();
    });
  </script>
</body>
</html>`;

const getVirtualDisplayBounds = () => {
  const displays = screen.getAllDisplays();
  let left = 0;
  let top = 0;
  let right = 0;
  let bottom = 0;
  let initialized = false;
  for (const display of displays) {
    const bounds = display?.bounds;
    if (!bounds) continue;
    if (!initialized) {
      left = bounds.x;
      top = bounds.y;
      right = bounds.x + bounds.width;
      bottom = bounds.y + bounds.height;
      initialized = true;
      continue;
    }
    left = Math.min(left, bounds.x);
    top = Math.min(top, bounds.y);
    right = Math.max(right, bounds.x + bounds.width);
    bottom = Math.max(bottom, bounds.y + bounds.height);
  }
  if (!initialized) {
    return { x: 0, y: 0, width: 1, height: 1 };
  }
  return { x: left, y: top, width: Math.max(1, right - left), height: Math.max(1, bottom - top) };
};

const ensureOverlayWindow = () => {
  if (!desktopEffectWindowsEnabled) {
    return null
  }
  if (overlayWindow && !overlayWindow.isDestroyed()) {
    return overlayWindow;
  }
  const bounds = getVirtualDisplayBounds();
  overlayWindow = new BrowserWindow({
    x: bounds.x,
    y: bounds.y,
    width: bounds.width,
    height: bounds.height,
    frame: false,
    show: false,
    transparent: true,
    resizable: false,
    movable: false,
    minimizable: false,
    maximizable: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    fullscreen: false,
    fullscreenable: false,
    focusable: false,
    hasShadow: false,
    backgroundColor: '#00000000',
    webPreferences: {
      nodeIntegration: true,
      contextIsolation: false,
      sandbox: false,
      devTools: false,
      backgroundThrottling: false
    }
  })
  overlayWindow.setMenuBarVisibility(false)
  overlayWindow.setAlwaysOnTop(true, 'screen-saver')
  overlayWindow.setIgnoreMouseEvents(true, { forward: true })
  overlayWindow.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true })
  overlayWindow.on('closed', () => {
    overlayWindow = null
  })
  overlayWindow.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(createOverlayHtml())}`)
  return overlayWindow
}

const updateOverlayBounds = () => {
  if (!overlayWindow || overlayWindow.isDestroyed()) {
    return
  }
  const bounds = getVirtualDisplayBounds()
  overlayWindow.setBounds(bounds, false)
}

const scheduleOverlayHide = (delayMs) => {
  if (overlayHideTimer) {
    clearTimeout(overlayHideTimer)
    overlayHideTimer = null
  }
  const delay = Math.max(0, Number(delayMs || 0))
  if (!delay) {
    return
  }
  overlayHideTimer = setTimeout(() => {
    overlayHideTimer = null
    hideOverlayNow()
  }, delay)
}

const hideOverlayNow = () => {
  if (overlayHideTimer) {
    clearTimeout(overlayHideTimer)
    overlayHideTimer = null
  }
  if (!overlayWindow || overlayWindow.isDestroyed()) {
    return
  }
  overlayWindow.webContents.send(OVERLAY_HIDE_CHANNEL)
  overlayWindow.hide()
}

const sendOverlayPayload = (payload) => {
  const window = ensureOverlayWindow()
  if (!window || window.isDestroyed()) {
    return false
  }
  updateOverlayBounds()
  window.webContents.send(OVERLAY_UPDATE_CHANNEL, payload)
  if (!window.isVisible()) {
    window.showInactive()
  }
  return true
}

const resolveOverlayPoint = (x, y) => {
  const primary = screen.getPrimaryDisplay()
  const scale = primary?.scaleFactor || 1
  const logicalX = Number(x) / scale
  const logicalY = Number(y) / scale
  const bounds = getVirtualDisplayBounds()
  const baseX = primary?.bounds?.x || 0
  const baseY = primary?.bounds?.y || 0
  return {
    x: Math.round(baseX + logicalX - bounds.x),
    y: Math.round(baseY + logicalY - bounds.y)
  }
}

const showControllerOverlay = (payload, state) => {
  const x = Number(payload?.x)
  const y = Number(payload?.y)
  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    return false
  }
  const point = resolveOverlayPoint(x, y)
  const description = String(payload?.description || '').trim()
  const duration = Number(payload?.durationMs ?? payload?.duration_ms)
  const delayMs = Number.isFinite(duration) && duration > 0 ? duration : state === 'done' ? DEFAULT_OVERLAY_DONE_MS : DEFAULT_OVERLAY_HINT_MS
  const shown = sendOverlayPayload({
    mode: 'controller',
    state,
    x: point.x,
    y: point.y,
    description
  })
  if (!shown) {
    return false
  }
  scheduleOverlayHide(delayMs)
  return true
}

const showMonitorOverlay = (payload) => {
  const waitMs = Math.max(0, Number(payload?.waitMs ?? payload?.wait_ms ?? 0))
  const shown = sendOverlayPayload({ mode: 'monitor', waitMs })
  if (!shown) {
    return false
  }
  scheduleOverlayHide(waitMs > 0 ? waitMs : DEFAULT_OVERLAY_MIN_HIDE_MS)
  return true
}

const createCompanionHtml = () => `<!doctype html>
<html>
<head>
<meta charset="utf-8" />
<style>
  html, body {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    background: transparent;
    overflow: hidden;
    font-family: "Segoe UI", "Helvetica Neue", Arial, sans-serif;
    user-select: none;
    pointer-events: none;
  }
  #root {
    position: relative;
    width: 100%;
    height: 100%;
    cursor: default;
    pointer-events: none;
  }
  #root.dragging { cursor: grabbing; }
  #sprite.dragging { cursor: grabbing; }
  #bubble {
    position: absolute;
    left: 50%;
    bottom: calc(100% - 4px);
    transform: translateX(-50%);
    width: max-content;
    min-width: 88px;
    max-width: min(320px, calc(100vw - 24px));
    padding: 8px 10px;
    border: 1px solid rgba(37, 99, 235, 0.22);
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.96);
    box-shadow: 0 10px 28px rgba(15, 23, 42, 0.16);
    color: #1f2937;
    font-size: 13px;
    line-height: 1.45;
    text-align: center;
    white-space: normal;
    overflow-wrap: anywhere;
    box-sizing: border-box;
    pointer-events: none;
    z-index: 1;
  }
  #bubble.success {
    border-color: rgba(20, 184, 166, 0.28);
    color: #0f766e;
  }
  #bubble.warning {
    border-color: rgba(245, 158, 11, 0.3);
    color: #92400e;
  }
  #sprite {
    position: absolute;
    left: 50%;
    bottom: 0;
    transform: translateX(-50%);
    overflow: hidden;
    cursor: pointer;
    pointer-events: auto;
    touch-action: none;
    -ms-touch-action: none;
  }
  #root.dragging #sprite { cursor: grabbing; }
  #sheet {
    position: absolute;
    left: 0;
    top: 0;
    width: 192px;
    height: 208px;
    background-repeat: no-repeat;
    transform-origin: left top;
    pointer-events: none;
  }
  .hidden { display: none; }
  .menu {
    position: fixed;
    z-index: 20;
    min-width: 180px;
    padding: 8px;
    border: 1px solid rgba(148, 163, 184, 0.22);
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.98);
    box-shadow: 0 18px 42px rgba(15, 23, 42, 0.18);
    display: none;
    flex-direction: column;
    gap: 6px;
    pointer-events: auto;
    -webkit-app-region: no-drag;
  }
  .menu.open { display: flex; }
  .menu button,
  .menu .scale {
    border: 0;
    border-radius: 10px;
    background: transparent;
    color: #0f172a;
    text-align: left;
    cursor: pointer;
  }
  .menu button {
    padding: 9px 10px;
    font-size: 13px;
  }
  .menu button:hover,
  .menu .scale:hover {
    background: rgba(59, 130, 246, 0.08);
  }
  .menu__group {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 2px 2px;
  }
  .menu__label {
    font-size: 12px;
    font-weight: 600;
    color: #64748b;
  }
  .menu__scales {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .menu .scale {
    padding: 6px 8px;
    font-size: 12px;
  }
  .menu .scale.active {
    background: rgba(59, 130, 246, 0.12);
    color: #1d4ed8;
  }
</style>
</head>
<body>
  <div id="root">
    <div id="bubble" class="hidden"></div>
    <div id="sprite"><div id="sheet"></div></div>
    <div id="menu" class="menu" aria-hidden="true">
      <button id="open-chat" type="button"></button>
      <button id="toggle-visible" type="button"></button>
      <div class="menu__group">
        <span id="scale-label" class="menu__label"></span>
        <div id="scale-row" class="menu__scales"></div>
      </div>
    </div>
  </div>
  <script>
    const { ipcRenderer } = require('electron');
    const root = document.getElementById('root');
    const bubble = document.getElementById('bubble');
    const sprite = document.getElementById('sprite');
    const sheet = document.getElementById('sheet');
    const menu = document.getElementById('menu');
    const openChatButton = document.getElementById('open-chat');
    const toggleVisibleButton = document.getElementById('toggle-visible');
    const scaleLabel = document.getElementById('scale-label');
    const scaleRow = document.getElementById('scale-row');
    const frameWidth = 192;
    const frameHeight = 208;
    const scalePresets = [0.5, 0.8, 1.0, 1.2, 1.4, 1.6];
    const menuTexts = {
      openChat: '进入聊天',
      hide: '隐藏',
      scale: '大小'
    };
    const states = {
      idle: { row: 0, frames: 6, duration: 1100 },
      'running-right': { row: 1, frames: 8, duration: 1060 },
      'running-left': { row: 2, frames: 8, duration: 1060 },
      waving: { row: 3, frames: 4, duration: 700 },
      jumping: { row: 4, frames: 5, duration: 840 },
      failed: { row: 5, frames: 8, duration: 1220 },
      waiting: { row: 6, frames: 6, duration: 1010 },
      running: { row: 7, frames: 6, duration: 820 },
      review: { row: 8, frames: 6, duration: 1030 }
    };
    let payload = {};
    let frame = 0;
    let timer = null;
    let drag = null;
    let suppressClick = false;
    let hasDragged = false;
    let animationSignature = '';
    let baseState = 'idle';
    let menuState = false;
    let currentScale = 1;
    let lastPosition = { x: 0, y: 0 };
    let currentPayload = {};
    let dragFrameId = null;
    let pendingDragDelta = { dx: 0, dy: 0 };
    let pendingDragTarget = null;
    let lastPointerClientPoint = null;
    let hitCanvas = null;
    let hitContext = null;
    let hitImage = null;
    let hitImageSource = '';
    let hitImageReady = false;
    let shapeFrameSignature = '';
    let shapeSupported = true;
    let shapeSuspended = false;
    let mouseTransparent = false;
    const hitAlphaThreshold = 12;
    const shapeStride = 3;
    const shapeMaxRects = 900;
    const dragActivateDistance = 4;
    const dragDirectionThreshold = 2;
    const resolveWindowPoint = () => ({
      x: Number(window.screenX || window.screenLeft || 0),
      y: Number(window.screenY || window.screenTop || 0)
    });
    const readClientPoint = (event) => {
      const x = Number(event?.clientX);
      const y = Number(event?.clientY);
      return Number.isFinite(x) && Number.isFinite(y) ? { x, y } : null;
    };
    const resolveClientPoint = (event) => readClientPoint(event) || { x: 0, y: 0 };
    const rememberPointerClientPoint = (event) => {
      const point = readClientPoint(event);
      if (!point) return;
      lastPointerClientPoint = {
        x: point.x,
        y: point.y,
        at: Date.now()
      };
    };
    const resolvePointerPoint = (event) => {
      const windowPoint = resolveWindowPoint();
      const clientPoint = resolveClientPoint(event);
      const fallback = {
        x: windowPoint.x + clientPoint.x,
        y: windowPoint.y + clientPoint.y
      };
      const screenX = Number(event?.screenX);
      const screenY = Number(event?.screenY);
      if (Number.isFinite(screenX) && Number.isFinite(screenY)) {
        if (event?.pointerType === 'touch') {
          const drift = Math.hypot(screenX - fallback.x, screenY - fallback.y);
          return drift < 96 ? { x: screenX, y: screenY } : fallback;
        }
        return { x: screenX, y: screenY };
      }
      return fallback;
    };
    const resolveMenuPoint = (event) => {
      const rawPoint = readClientPoint(event);
      const hasRecentPointerPoint = lastPointerClientPoint && Date.now() - lastPointerClientPoint.at < 1600;
      const clientPoint = (!rawPoint || (rawPoint.x === 0 && rawPoint.y === 0 && hasRecentPointerPoint))
        ? lastPointerClientPoint || { x: 0, y: 0 }
        : rawPoint;
      return {
        x: Math.max(0, Math.min(Math.round(clientPoint.x), Math.max(0, window.innerWidth - 1))),
        y: Math.max(0, Math.min(Math.round(clientPoint.y), Math.max(0, window.innerHeight - 1)))
      };
    };

    const normalizeState = (value) => states[value] ? value : 'idle';
    const identityPayload = (extra) => Object.assign({
      key: currentPayload.key || currentPayload.selectedId,
      agentId: currentPayload.agentId,
      selectedId: currentPayload.selectedId
    }, extra || {});
    const sendCommand = (action, extra) => {
      ipcRenderer.invoke('${COMPANION_COMMAND_CHANNEL}', Object.assign({ action }, extra || {})).catch(() => {});
    };
    const setMouseTransparent = (transparent) => {
      if (mouseTransparent === transparent) return;
      mouseTransparent = transparent;
      ipcRenderer.invoke('wunder:companion-pointer-events', identityPayload({ ignore: transparent })).catch(() => {});
    };
    const ensureHitCanvas = () => {
      if (!hitCanvas) {
        hitCanvas = document.createElement('canvas');
        hitCanvas.width = frameWidth;
        hitCanvas.height = frameHeight;
        hitContext = hitCanvas.getContext('2d', { willReadFrequently: true });
      }
      return hitContext;
    };
    const clearHitImage = () => {
      hitImage = null;
      hitImageSource = '';
      hitImageReady = false;
      shapeFrameSignature = '';
    };
    const updateHitImage = () => {
      const source = String(payload.spritesheetDataUrl || '');
      if (!source) {
        clearHitImage();
        return;
      }
      if (source === hitImageSource) {
        return;
      }
      hitImageSource = source;
      hitImageReady = false;
      hitImage = new Image();
      hitImage.onload = () => {
        hitImageReady = true;
        shapeFrameSignature = '';
        syncHitShape();
      };
      hitImage.onerror = clearHitImage;
      hitImage.src = source;
    };
    const resolveFullShapeRects = () => {
      const scale = Math.min(1.6, Math.max(0.5, Number(payload.scale || 1)));
      return [{
        x: 0,
        y: 0,
        width: Math.max(1, Math.round(frameWidth * scale)),
        height: Math.max(1, Math.round(frameHeight * scale))
      }];
    };
    const resetHitShape = () => {
      shapeFrameSignature = '';
      if (!shapeSupported) return;
      ipcRenderer.invoke('wunder:companion-hit-shape', identityPayload({ rects: resolveFullShapeRects() })).then((supported) => {
        if (supported === false) shapeSupported = false;
      }).catch(() => {
        shapeSupported = false;
      });
    };
    const buildFrameShapeRects = () => {
      if (!hitImageReady || !hitImage) return null;
      const context = ensureHitCanvas();
      if (!context) return null;
      const state = states[normalizeState(payload.state)];
      const scale = Math.min(1.6, Math.max(0.5, Number(payload.scale || 1)));
      const width = Math.max(1, Math.round(frameWidth * scale));
      const height = Math.max(1, Math.round(frameHeight * scale));
      const sourceX = frame * frameWidth;
      const sourceY = state.row * frameHeight;
      try {
        if (hitCanvas.width !== frameWidth || hitCanvas.height !== frameHeight) {
          hitCanvas.width = frameWidth;
          hitCanvas.height = frameHeight;
        }
        context.clearRect(0, 0, frameWidth, frameHeight);
        context.drawImage(hitImage, sourceX, sourceY, frameWidth, frameHeight, 0, 0, frameWidth, frameHeight);
        const data = context.getImageData(0, 0, frameWidth, frameHeight).data;
        const rects = [];
        for (let y = 0; y < frameHeight; y += shapeStride) {
          let start = -1;
          for (let x = 0; x <= frameWidth; x += shapeStride) {
            const sampleX = Math.min(frameWidth - 1, x);
            const sampleY = Math.min(frameHeight - 1, y);
            const alpha = x < frameWidth ? data[(sampleY * frameWidth + sampleX) * 4 + 3] : 0;
            if (alpha >= hitAlphaThreshold) {
              if (start < 0) start = x;
            } else if (start >= 0) {
              rects.push({
                x: Math.max(0, Math.floor(start * scale)),
                y: Math.max(0, Math.floor(y * scale)),
                width: Math.max(1, Math.ceil((x - start + shapeStride) * scale)),
                height: Math.max(1, Math.ceil(shapeStride * scale))
              });
              start = -1;
              if (rects.length >= shapeMaxRects) return rects;
            }
          }
        }
        return rects.length ? rects : [{ x: 0, y: 0, width, height }];
      } catch {
        return null;
      }
    };
    const syncHitShape = () => {
      if (!shapeSupported || shapeSuspended) return;
      if (!hitImageReady || !hitImage) {
        resetHitShape();
        return;
      }
      const signature = [
        String(payload.spritesheetDataUrl || ''),
        normalizeState(payload.state),
        String(frame),
        String(payload.scale || 1)
      ].join('|');
      if (signature === shapeFrameSignature) return;
      shapeFrameSignature = signature;
      const rects = buildFrameShapeRects();
      if (!rects) return;
      ipcRenderer.invoke('wunder:companion-hit-shape', identityPayload({ rects })).then((supported) => {
        if (supported === false) shapeSupported = false;
      }).catch(() => {
        shapeSupported = false;
      });
    };
    const isOpaqueSpritePoint = (clientX, clientY) => {
      const rect = sprite.getBoundingClientRect();
      if (!rect.width || !rect.height) return false;
      if (clientX < rect.left || clientX > rect.right || clientY < rect.top || clientY > rect.bottom) {
        return false;
      }
      if (!hitImageReady || !hitImage) {
        return true;
      }
      const context = ensureHitCanvas();
      if (!context) {
        return true;
      }
      const state = states[normalizeState(payload.state)];
      const sourceX = Math.max(0, Math.min(frameWidth - 1, frame * frameWidth));
      const sourceY = Math.max(0, Math.min(frameHeight - 1, state.row * frameHeight));
      const localX = Math.max(0, Math.min(frameWidth - 1, Math.floor((clientX - rect.left) * frameWidth / rect.width)));
      const localY = Math.max(0, Math.min(frameHeight - 1, Math.floor((clientY - rect.top) * frameHeight / rect.height)));
      try {
        context.clearRect(0, 0, 1, 1);
        context.drawImage(hitImage, sourceX + localX, sourceY + localY, 1, 1, 0, 0, 1, 1);
        return context.getImageData(0, 0, 1, 1).data[3] >= hitAlphaThreshold;
      } catch {
        return true;
      }
    };
    const isOpaquePointerEvent = (event) => {
      const point = readClientPoint(event);
      if (!point) return true;
      return isOpaqueSpritePoint(point.x, point.y);
    };
    const clampScale = (value) => Math.min(1.6, Math.max(0.5, Number(value) || 1));
    const applyFrame = () => {
      const stateKey = normalizeState(payload.state);
      const state = states[stateKey];
      sheet.style.backgroundPosition = '-' + (frame * frameWidth) + 'px -' + (state.row * frameHeight) + 'px';
      syncHitShape();
    };
    const stopAnimation = () => {
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
    };
    const syncMenu = () => {
      const hidden = !menuState;
      menu.classList.toggle('open', !hidden);
      menu.setAttribute('aria-hidden', hidden ? 'true' : 'false');
      if (hidden) return;
      const rect = menu.getBoundingClientRect();
      const left = Math.max(8, Math.min(lastPosition.x, window.innerWidth - rect.width - 8));
      const top = Math.max(8, Math.min(lastPosition.y, window.innerHeight - rect.height - 8));
      menu.style.left = left + 'px';
      menu.style.top = top + 'px';
    };
    const closeMenu = () => {
      menuState = false;
      syncMenu();
    };
    const openMenu = (clientX, clientY) => {
      lastPosition = { x: Math.max(8, clientX), y: Math.max(8, clientY) };
      menuState = true;
      syncMenu();
    };
    const buildScaleButtons = () => {
      scaleRow.innerHTML = '';
      scalePresets.forEach((value) => {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'scale' + (Math.abs(currentScale - value) < 0.001 ? ' active' : '');
        button.textContent = value.toFixed(1) + 'x';
        button.addEventListener('click', () => {
          sendCommand('set-scale', {
            scale: value,
            key: currentPayload.key || currentPayload.selectedId,
            agentId: currentPayload.agentId
          });
        });
        scaleRow.appendChild(button);
      });
    };
    const renderControls = () => {
      currentScale = clampScale(payload.scale);
      openChatButton.textContent = menuTexts.openChat;
      toggleVisibleButton.textContent = menuTexts.hide;
      scaleLabel.textContent = menuTexts.scale;
      buildScaleButtons();
    };
    const setDragging = (active) => {
      root.classList.toggle('dragging', active);
      sprite.classList.toggle('dragging', active);
    };
    const applyLocalState = (state) => {
      payload.state = normalizeState(state);
      refreshAnimationIfNeeded();
    };
    const applyDragState = (dx) => {
      if (Math.abs(dx) < dragDirectionThreshold) {
        return;
      }
      applyLocalState(dx < 0 ? 'running-left' : 'running-right');
    };
    const resetDragState = () => {
      applyLocalState(baseState);
    };
    const flushDragFrame = () => {
      dragFrameId = null;
      if (!drag) {
        return;
      }
      const dx = pendingDragDelta.dx;
      const dy = pendingDragDelta.dy;
      const target = pendingDragTarget;
      pendingDragDelta = { dx: 0, dy: 0 };
      pendingDragTarget = null;
      if (!target && dx === 0 && dy === 0) {
        return;
      }
      applyDragState(dx);
      ipcRenderer.invoke('wunder:companion-drag', identityPayload(target ? { x: target.x, y: target.y, dx, dy } : { dx, dy })).catch(() => {});
    };
    const queueDragFrame = (targetX, targetY, dx, dy) => {
      pendingDragDelta.dx += dx;
      pendingDragDelta.dy += dy;
      pendingDragTarget = { x: targetX, y: targetY };
      if (dragFrameId !== null || typeof window === 'undefined') {
        return;
      }
      dragFrameId = window.requestAnimationFrame(flushDragFrame);
    };
    const startAnimation = () => {
      stopAnimation();
      frame = 0;
      applyFrame();
      const state = states[normalizeState(payload.state)];
      const frameMs = Math.max(50, Math.round(state.duration / Math.max(1, state.frames)));
      timer = setInterval(() => {
        frame = (frame + 1) % state.frames;
        applyFrame();
      }, frameMs);
    };
    const refreshAnimationIfNeeded = () => {
      const nextSignature = [
        normalizeState(payload.state),
        String(payload.spritesheetDataUrl || '')
      ].join('|');
      if (nextSignature === animationSignature) {
        return;
      }
      animationSignature = nextSignature;
      startAnimation();
    };
    const render = (next) => {
      payload = Object.assign({}, payload, next || {});
      currentPayload = payload;
      baseState = normalizeState(payload.state);
      mouseTransparent = false;
      const scale = Math.min(1.6, Math.max(0.5, Number(payload.scale || 1)));
      sprite.style.width = Math.round(frameWidth * scale) + 'px';
      sprite.style.height = Math.round(frameHeight * scale) + 'px';
      sheet.style.backgroundImage = payload.spritesheetDataUrl ? 'url("' + payload.spritesheetDataUrl + '")' : '';
      sheet.style.transform = 'scale(' + Math.round(scale * 1000) / 1000 + ')';
      updateHitImage();
      shapeFrameSignature = '';
      const text = String(payload.message || '').trim();
      if (text && payload.messageVisible) {
        bubble.textContent = text;
        bubble.className = String(payload.messageKind || 'info').trim();
      } else {
        bubble.textContent = '';
        bubble.className = 'hidden';
      }
      renderControls();
      refreshAnimationIfNeeded();
    };
    ipcRenderer.on('wunder:companion-render', (event, next) => render(next));
    openChatButton.addEventListener('click', () => {
      closeMenu();
      sendCommand('open-chat', {
        key: currentPayload.key || currentPayload.selectedId,
        agentId: currentPayload.agentId
      });
    });
    toggleVisibleButton.addEventListener('click', () => {
      closeMenu();
      sendCommand('hide', {
        key: currentPayload.key || currentPayload.selectedId,
        agentId: currentPayload.agentId
      });
    });
    sprite.addEventListener('contextmenu', (event) => {
      if (!isOpaquePointerEvent(event)) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      if (drag) {
        suppressClick = true;
        stopDrag(event);
      }
      const menuPoint = resolveMenuPoint(event);
      sendCommand('context-menu', {
        key: currentPayload.key || currentPayload.selectedId,
        agentId: currentPayload.agentId,
        x: menuPoint.x,
        y: menuPoint.y
      });
    });
    window.addEventListener('mousedown', (event) => {
      if (!menuState) return;
      if (event.target && menu.contains(event.target)) return;
      closeMenu();
    });
    window.addEventListener('resize', syncMenu);
    window.addEventListener('blur', closeMenu);
    sprite.addEventListener('pointerdown', (event) => {
      if (event.button !== 0) return;
      if (!isOpaquePointerEvent(event)) {
        setMouseTransparent(true);
        return;
      }
      if (drag) return;
      if (event.pointerType === 'touch' && event.isPrimary === false) return;
      if (menuState) {
        closeMenu();
      }
      rememberPointerClientPoint(event);
      const point = resolvePointerPoint(event);
      const windowPoint = resolveWindowPoint();
      drag = {
        pointerId: event.pointerId,
        startX: point.x,
        startY: point.y,
        x: point.x,
        y: point.y,
        grabX: point.x - windowPoint.x,
        grabY: point.y - windowPoint.y,
        targetX: windowPoint.x,
        targetY: windowPoint.y
      };
      pendingDragDelta = { dx: 0, dy: 0 };
      pendingDragTarget = null;
      suppressClick = false;
      hasDragged = false;
      shapeSuspended = true;
      resetHitShape();
      setMouseTransparent(false);
      setDragging(true);
      sprite.setPointerCapture(event.pointerId);
    });
    sprite.addEventListener('pointermove', (event) => {
      if (!drag && !menuState) {
        setMouseTransparent(!isOpaquePointerEvent(event));
        return;
      }
      if (!drag) return;
      if (event.pointerId !== drag.pointerId) return;
      event.preventDefault();
      rememberPointerClientPoint(event);
      const point = resolvePointerPoint(event);
      const dx = point.x - drag.x;
      const dy = point.y - drag.y;
      drag.x = point.x;
      drag.y = point.y;
      if (!suppressClick && Math.hypot(point.x - drag.startX, point.y - drag.startY) > dragActivateDistance) {
        suppressClick = true;
        hasDragged = true;
      }
      const targetX = Math.round(point.x - drag.grabX);
      const targetY = Math.round(point.y - drag.grabY);
      const targetDx = targetX - drag.targetX;
      const targetDy = targetY - drag.targetY;
      if (targetDx === 0 && targetDy === 0) {
        return;
      }
      drag.targetX = targetX;
      drag.targetY = targetY;
      queueDragFrame(targetX, targetY, targetDx, targetDy);
    });
    sprite.addEventListener('click', (event) => {
      if (!isOpaquePointerEvent(event)) {
        event.preventDefault();
        event.stopPropagation();
        setMouseTransparent(true);
        return;
      }
      if (suppressClick) {
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (event.detail >= 2) {
        sendCommand('open-chat', {
          key: currentPayload.key || currentPayload.selectedId,
          agentId: currentPayload.agentId
        });
        return;
      }
      sendCommand('wave', {
        key: currentPayload.key || currentPayload.selectedId,
        agentId: currentPayload.agentId
      });
    });
    sprite.addEventListener('wheel', (event) => {
      if (!event.altKey) return;
      if (!isOpaquePointerEvent(event)) return;
      event.preventDefault();
      const nextScale = clampScale(currentScale + (event.deltaY > 0 ? -0.1 : 0.1));
      sendCommand('set-scale', {
        scale: nextScale,
        key: currentPayload.key || currentPayload.selectedId,
        agentId: currentPayload.agentId
      });
    }, { passive: false });
    const stopDrag = (event) => {
      if (!drag) return;
      if (event && event.pointerId !== undefined && event.pointerId !== drag.pointerId) return;
      if (dragFrameId !== null && typeof window !== 'undefined') {
        window.cancelAnimationFrame(dragFrameId);
        dragFrameId = null;
      }
      const dx = pendingDragDelta.dx;
      const dy = pendingDragDelta.dy;
      const target = pendingDragTarget;
      pendingDragDelta = { dx: 0, dy: 0 };
      pendingDragTarget = null;
      if (target || dx !== 0 || dy !== 0) {
        ipcRenderer.invoke('wunder:companion-drag', identityPayload(target ? { x: target.x, y: target.y, dx, dy } : { dx, dy })).catch(() => {});
      }
      const pointerId = drag.pointerId;
      drag = null;
      setDragging(false);
      setMouseTransparent(false);
      if (pointerId !== undefined) {
        try { sprite.releasePointerCapture(pointerId); } catch {}
      }
      resetDragState();
      shapeSuspended = false;
      shapeFrameSignature = '';
      syncHitShape();
      if (hasDragged) {
        ipcRenderer.invoke('wunder:companion-drag-end', identityPayload()).catch(() => {});
      }
      window.setTimeout(() => {
        suppressClick = false;
        hasDragged = false;
      }, 180);
    };
    sprite.addEventListener('pointerup', stopDrag);
    sprite.addEventListener('pointercancel', stopDrag);
    sprite.addEventListener('lostpointercapture', stopDrag);
    window.addEventListener('pointerup', stopDrag);
    window.addEventListener('pointercancel', stopDrag);
    window.addEventListener('mouseup', stopDrag);
    window.addEventListener('blur', stopDrag);
    window.addEventListener('beforeunload', () => {
      if (dragFrameId !== null && typeof window !== 'undefined') {
        window.cancelAnimationFrame(dragFrameId);
        dragFrameId = null;
      }
      stopDrag();
      setMouseTransparent(false);
      stopAnimation();
    });
  </script>
</body>
</html>`

const resolveCompanionWindowSize = (state) => {
  const scale = Number(state?.scale || 1)
  const safeScale = Number.isFinite(scale) ? Math.min(COMPANION_MAX_SCALE, Math.max(COMPANION_MIN_SCALE, scale)) : 1
  return {
    width: Math.max(1, Math.round(COMPANION_FRAME_WIDTH * safeScale)),
    height: Math.max(1, Math.round(COMPANION_FRAME_HEIGHT * safeScale))
  }
}

const clampCompanionBounds = (x, y, size) => {
  const bounds = getVirtualDisplayBounds()
  const minX = bounds.x + COMPANION_SCREEN_MARGIN
  const minY = bounds.y + COMPANION_SCREEN_MARGIN
  const maxX = Math.max(
    minX,
    bounds.x + bounds.width - Number(size?.width || 0) - COMPANION_SCREEN_MARGIN
  )
  const maxY = Math.max(
    minY,
    bounds.y + bounds.height - Number(size?.height || 0) - COMPANION_SCREEN_MARGIN
  )
  return {
    x: Math.min(Math.max(minX, Math.round(x)), maxX),
    y: Math.min(Math.max(minY, Math.round(y)), maxY)
  }
}

const resolveCompanionWindowBounds = (state) => {
  const size = resolveCompanionWindowSize(state)
  const point = clampCompanionBounds(state?.x, state?.y, size)
  return {
    point,
    size,
    bounds: { x: point.x, y: point.y, width: size.width, height: size.height }
  }
}

const hasCompanionRuntimeIdentity = (payload = {}) => {
  const source = payload && typeof payload === 'object' ? payload : {}
  return Boolean(
    String(source.key || '').trim() ||
    String(source.agentId || source.agent_id || '').trim() ||
    String(source.selectedId || source.selected_id || source.id || '').trim()
  )
}

const findCompanionRuntimeByWebContents = (webContents) => {
  if (!webContents) {
    return null
  }
  return Array.from(companionRuntimes.values()).find((runtime) =>
    runtime.window && !runtime.window.isDestroyed() && runtime.window.webContents.id === webContents.id
  ) || null
}

const resolveCompanionRuntimeForIpc = (event, payload = {}, create = false) => {
  if (hasCompanionRuntimeIdentity(payload)) {
    return create ? getCompanionRuntime(payload) : findCompanionRuntime(payload)
  }
  const bySender = findCompanionRuntimeByWebContents(event?.sender)
  if (bySender) {
    return bySender
  }
  return create ? getCompanionRuntime(payload) : findCompanionRuntime(payload)
}

const syncLegacyCompanionGlobals = (runtime) => {
  if (!runtime) {
    return
  }
  rememberPrimaryCompanionRuntime(runtime)
}

const setCompanionPointerEvents = (payload = {}, event = null) => {
  const runtime = resolveCompanionRuntimeForIpc(event, payload)
  if (!runtime?.window || runtime.window.isDestroyed()) {
    return false
  }
  const ignore = payload?.ignore === true
  if (runtime.mouseIgnored === ignore) {
    return true
  }
  runtime.mouseIgnored = ignore
  runtime.window.setIgnoreMouseEvents(ignore, ignore ? { forward: true } : undefined)
  syncLegacyCompanionGlobals(runtime)
  return true
}

const resetCompanionWindowInput = (runtime) => {
  if (!runtime?.window || runtime.window.isDestroyed()) {
    return false
  }
  try {
    runtime.window.setIgnoreMouseEvents(false)
    runtime.mouseIgnored = false
  } catch {
    runtime.mouseIgnored = false
  }
  if (typeof runtime.window.setShape === 'function') {
    try {
      const size = resolveCompanionWindowSize(runtime.state)
      runtime.window.setShape([{ x: 0, y: 0, width: size.width, height: size.height }])
      runtime.shapeEnabled = false
    } catch {
      runtime.shapeEnabled = false
    }
  }
  syncLegacyCompanionGlobals(runtime)
  return true
}

const updateCompanionHitShape = (payload = {}, event = null) => {
  const runtime = resolveCompanionRuntimeForIpc(event, payload)
  if (!runtime?.window || runtime.window.isDestroyed()) {
    return false
  }
  if (typeof runtime.window.setShape !== 'function') {
    runtime.shapeEnabled = false
    syncLegacyCompanionGlobals(runtime)
    return false
  }
  const size = resolveCompanionWindowSize(runtime.state)
  const sourceRects = Array.isArray(payload?.rects) ? payload.rects : []
  const rects = sourceRects
    .map((rect) => {
      const x = Number(rect?.x)
      const y = Number(rect?.y)
      const width = Number(rect?.width)
      const height = Number(rect?.height)
      if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(width) || !Number.isFinite(height)) {
        return null
      }
      const left = Math.max(0, Math.min(size.width, Math.round(x)))
      const top = Math.max(0, Math.min(size.height, Math.round(y)))
      const right = Math.max(left, Math.min(size.width, Math.round(x + width)))
      const bottom = Math.max(top, Math.min(size.height, Math.round(y + height)))
      if (right <= left || bottom <= top) {
        return null
      }
      return { x: left, y: top, width: right - left, height: bottom - top }
    })
    .filter(Boolean)
  try {
    runtime.window.setShape(rects.length ? rects : [{ x: 0, y: 0, width: size.width, height: size.height }])
    runtime.shapeEnabled = true
    syncLegacyCompanionGlobals(runtime)
    return true
  } catch {
    runtime.shapeEnabled = false
    syncLegacyCompanionGlobals(runtime)
    return false
  }
}

const ensureCompanionWindow = (runtime = getCompanionRuntime(companionState)) => {
  if (!desktopEffectWindowsEnabled) {
    return null
  }
  if (runtime.window && !runtime.window.isDestroyed()) {
    syncLegacyCompanionGlobals(runtime)
    return runtime.window
  }
  const windowLayout = resolveCompanionWindowBounds(runtime.state)
  const window = new BrowserWindow({
    x: windowLayout.bounds.x,
    y: windowLayout.bounds.y,
    width: windowLayout.bounds.width,
    height: windowLayout.bounds.height,
    frame: false,
    show: false,
    transparent: true,
    resizable: false,
    minimizable: false,
    maximizable: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    fullscreen: false,
    fullscreenable: false,
    focusable: false,
    hasShadow: false,
    backgroundColor: '#00000000',
    webPreferences: {
      nodeIntegration: true,
      contextIsolation: false,
      sandbox: false,
      devTools: false,
      backgroundThrottling: false
    }
  })
  runtime.window = window
  runtime.mouseIgnored = false
  runtime.shapeEnabled = false
  window.setMenuBarVisibility(false)
  window.setAlwaysOnTop(true, 'screen-saver')
  window.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true })
  window.on('closed', () => {
    clearRuntimeTransientState(runtime)
    runtime.window = null
    runtime.mouseIgnored = false
    runtime.shapeEnabled = false
    companionRuntimes.delete(runtime.key)
    if (companionWindow === window) {
      companionWindow = null
      companionWindowMouseIgnored = false
      companionWindowShapeEnabled = false
    }
  })
  window.webContents.once('did-finish-load', () => {
    renderCompanionWindow(runtime)
  })
  window.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(createCompanionHtml())}`)
  syncLegacyCompanionGlobals(runtime)
  return window
}

const renderCompanionWindow = (target = null) => {
  const runtime = target?.state ? target : getCompanionRuntime(target || companionState)
  const state = runtime.state
  if (!desktopEffectWindowsEnabled) {
    if (runtime.window && !runtime.window.isDestroyed()) {
      runtime.window.hide()
    }
    return false
  }
  if (!state.enabled || !state.spritesheetDataUrl) {
    if (runtime.window && !runtime.window.isDestroyed()) {
      runtime.window.hide()
    }
    return false
  }
  const window = ensureCompanionWindow(runtime)
  if (!window || window.isDestroyed()) {
    return false
  }
  const windowLayout = resolveCompanionWindowBounds(state)
  window.setBounds(windowLayout.bounds, false)
  runtime.state = normalizeCompanionState({
    ...state,
    x: windowLayout.point.x,
    y: windowLayout.point.y
  })
  resetCompanionWindowInput(runtime)
  syncLegacyCompanionGlobals(runtime)
  if (window.webContents.isLoading()) {
    return true
  }
  const renderState = resolveRenderedCompanionState(runtime)
  window.webContents.send('wunder:companion-render', {
    ...runtime.state,
    key: runtime.key,
    state: renderState
  })
  if (!window.isVisible()) {
    window.showInactive()
  }
  return true
}

const showCompanion = (payload) => {
  const runtime = getCompanionRuntime(payload)
  const incomingState = normalizeCompanionBaseState(payload?.state)
  const next = normalizeCompanionState({
    ...runtime.state,
    key: payload?.key || runtime.key,
    agentId: payload?.agentId || payload?.agent_id || runtime.state.agentId,
    selectedId: payload?.selectedId || payload?.id || runtime.state.selectedId,
    displayName: payload?.displayName || runtime.state.displayName,
    description: payload?.description || runtime.state.description,
    spritesheetDataUrl: payload?.spritesheetDataUrl || runtime.state.spritesheetDataUrl,
    state: incomingState || runtime.state.state,
    scale: payload?.scale ?? runtime.state.scale,
    x: payload?.x ?? runtime.state.x,
    y: payload?.y ?? runtime.state.y,
    message: payload?.message || '',
    messageKind: payload?.messageKind || 'info',
    messageVisible: payload?.messageVisible === true,
    enabled: true
  })
  runtime.state = next
  syncLegacyCompanionGlobals(runtime)
  if (payload?.persist !== false) {
    saveCompanionState(next)
  }
  return renderCompanionWindow(runtime)
}

const updateCompanion = (payload) => showCompanion(payload)

const hideCompanion = (payload = {}) => {
  const keepKeys = normalizeCompanionKeepKeys(payload?.keepKeys || payload?.keep_keys)
  if (keepKeys.size) {
    Array.from(companionRuntimes.values())
      .filter((runtime) => !keepKeys.has(runtime.key))
      .forEach((runtime) => {
        clearRuntimeTransientState(runtime)
        resetCompanionWindowInput(runtime)
        runtime.state = normalizeCompanionState({ ...runtime.state, enabled: false })
        if (runtime.window && !runtime.window.isDestroyed()) {
          runtime.window.hide()
        }
        syncLegacyCompanionGlobals(runtime)
      })
    if (payload?.persistEnabled === true || payload?.persistState === true) {
      writeJsonFile(resolveCompanionStatePath(), serializeCompanionStateFile())
    }
    return true
  }
  const shouldHideAll = !hasCompanionRuntimeIdentity(payload)
  const runtimes = shouldHideAll
    ? Array.from(companionRuntimes.values())
    : [findCompanionRuntime(payload)].filter(Boolean)
  const shouldPersistRuntimeState = payload?.persistEnabled === true || payload?.persistState === true
  let changedPersistentState = false
  runtimes.forEach((runtime) => {
    clearRuntimeTransientState(runtime)
    resetCompanionWindowInput(runtime)
    runtime.state = normalizeCompanionState({ ...runtime.state, enabled: false })
    changedPersistentState = shouldPersistRuntimeState
    if (runtime.window && !runtime.window.isDestroyed()) {
      runtime.window.hide()
    }
    syncLegacyCompanionGlobals(runtime)
  })
  if (shouldPersistRuntimeState) {
    const activeRuntime = Array.from(companionRuntimes.values()).find((runtime) =>
      normalizeCompanionState(runtime.state).enabled
    )
    if (activeRuntime) {
      syncLegacyCompanionGlobals(activeRuntime)
    }
  }
  if (shouldHideAll && !runtimes.length) {
    clearCompanionTransientState()
    if (payload?.persistEnabled === true) {
      saveCompanionState({ enabled: false })
    }
  } else if (shouldPersistRuntimeState && changedPersistentState) {
    writeJsonFile(resolveCompanionStatePath(), serializeCompanionStateFile())
  }
  if (payload?.persistEnabled === true && mainWindow && !mainWindow.isDestroyed()) {
    runtimes.forEach((runtime) => {
      sendMainWindowMessage('wunder:companion-state-changed', runtime.state)
    })
  }
  return true
}

const showCompanionContextMenu = (payload = {}) => {
  const runtime = findCompanionRuntime(payload)
  if (!runtime?.window || runtime.window.isDestroyed()) {
    return false
  }
  const x = Number.isFinite(Number(payload?.x)) ? Math.max(0, Math.round(Number(payload.x))) : undefined
  const y = Number.isFinite(Number(payload?.y)) ? Math.max(0, Math.round(Number(payload.y))) : undefined
  const template = [
    {
      label: '进入聊天',
      click: () => emitCompanionCommand({ action: 'open-chat', key: runtime.key, agentId: runtime.state.agentId })
    },
    {
      label: '隐藏',
      click: () => emitCompanionCommand({ action: 'hide', key: runtime.key, agentId: runtime.state.agentId })
    }
  ]
  template[0].label = '\u8fdb\u5165\u804a\u5929'
  template[1].label = '\u9690\u85cf'
  Menu.buildFromTemplate(template).popup({
    window: runtime.window,
    x,
    y
  })
  return true
}

const emitCompanionCommand = (payload) => {
  const action = String(payload?.action || '').trim().toLowerCase()
  const runtime = action === 'context-menu'
    ? findCompanionRuntime(payload)
    : resolveCompanionRuntimeForIpc(null, payload)
  if (action === 'open-chat') {
    setCompanionTransientState(runtime?.state || payload, 'waving', 900)
    if (runtime) {
      renderCompanionWindow(runtime)
    }
    showMainWindow({ explicit: true })
    sendMainWindowMessage(COMPANION_COMMAND_CHANNEL, payload)
    return true
  }
  if (action === 'hide') {
    if (runtime) {
      hideCompanion({ key: runtime.key, persistEnabled: false })
    }
    if (mainWindow && !mainWindow.isDestroyed()) {
      sendMainWindowMessage(COMPANION_COMMAND_CHANNEL, payload)
    } else if (runtime) {
      hideCompanion({ key: runtime.key, persistEnabled: true })
    }
    return true
  }
  if (action === 'set-scale') {
    const scale = Number(payload?.scale)
    if (Number.isFinite(scale) && runtime) {
      runtime.state = normalizeCompanionState({
        ...runtime.state,
        scale,
        state: normalizeCompanionBaseState(runtime.state.state)
      })
      syncLegacyCompanionGlobals(runtime)
      renderCompanionWindow(runtime)
    }
    if (mainWindow && !mainWindow.isDestroyed()) {
      sendMainWindowMessage(COMPANION_COMMAND_CHANNEL, payload)
    }
    return true
  }
  if (action === 'wave') {
    setCompanionTransientState(runtime?.state || payload, 'waving', 1100)
    if (runtime) {
      renderCompanionWindow(runtime)
    }
    return true
  }
  if (action === 'context-menu') {
    return showCompanionContextMenu(payload)
  }
  if (mainWindow && !mainWindow.isDestroyed()) {
    sendMainWindowMessage(COMPANION_COMMAND_CHANNEL, payload)
  }
  return true
}

const moveCompanionBy = (payload, event = null) => {
  const runtime = resolveCompanionRuntimeForIpc(event, payload)
  if (!runtime?.window || runtime.window.isDestroyed()) {
    return false
  }
  const targetX = Number(payload?.x)
  const targetY = Number(payload?.y)
  const dx = Number(payload?.dx || 0)
  const dy = Number(payload?.dy || 0)
  const hasTarget = Number.isFinite(targetX) && Number.isFinite(targetY)
  const windowLayout = resolveCompanionWindowBounds({
    ...runtime.state,
    x: hasTarget ? targetX : runtime.state.x + dx,
    y: hasTarget ? targetY : runtime.state.y + dy
  })
  runtime.state = normalizeCompanionState({
    ...runtime.state,
    x: windowLayout.point.x,
    y: windowLayout.point.y
  })
  runtime.window.setBounds(windowLayout.bounds, false)
  syncLegacyCompanionGlobals(runtime)
  return true
}

const endCompanionDrag = (payload = {}, event = null) => {
  const runtime = resolveCompanionRuntimeForIpc(event, payload)
  if (!runtime?.window || runtime.window.isDestroyed()) {
    return false
  }
  const windowLayout = resolveCompanionWindowBounds(runtime.state)
  runtime.state = normalizeCompanionState({
    ...runtime.state,
    x: windowLayout.point.x,
    y: windowLayout.point.y,
    state: runtime.state.state === 'jumping' ? 'waiting' : normalizeCompanionBaseState(runtime.state.state)
  })
  clearRuntimeTransientState(runtime)
  saveCompanionState({
    key: runtime.key,
    agentId: runtime.state.agentId,
    selectedId: runtime.state.selectedId,
    enabled: runtime.state.enabled,
    x: windowLayout.point.x,
    y: windowLayout.point.y,
    state: runtime.state.state
  })
  syncLegacyCompanionGlobals(runtime)
  renderCompanionWindow(runtime)
  sendMainWindowMessage('wunder:companion-state-changed', {
    ...runtime.state,
    key: runtime.key,
    dragEnded: true
  })
  return true
}

const chromiumLogLevel = process.env.WUNDER_CHROMIUM_LOG_LEVEL || (suppressGpuWarnings ? '3' : '2')
app.commandLine.appendSwitch('log-level', chromiumLogLevel)
if (rendererCompatibilityModeEnabled) {
  app.commandLine.appendSwitch('disable-gpu')
  app.commandLine.appendSwitch('disable-gpu-compositing')
}
if (process.platform === 'linux') {
  app.commandLine.appendSwitch('class', 'wunder-desktop')
  if (suppressGpuWarnings) {
    if (!process.env.MESA_LOG_LEVEL) {
      process.env.MESA_LOG_LEVEL = 'error'
    }
    if (!process.env.MESA_DEBUG) {
      process.env.MESA_DEBUG = 'silent'
    }
    if (!process.env.LIBGL_DEBUG) {
      process.env.LIBGL_DEBUG = 'quiet'
    }
    if (!process.env.EGL_LOG_LEVEL) {
      process.env.EGL_LOG_LEVEL = 'error'
    }
    if (!process.env.VK_LOADER_DEBUG) {
      process.env.VK_LOADER_DEBUG = 'error'
    }
    app.commandLine.appendSwitch('disable-vulkan')
    app.commandLine.appendSwitch('disable-features', 'Vulkan')
  }
}
if (disableElectronHardwareAcceleration) {
  app.disableHardwareAcceleration()
}

const repoRoot = path.resolve(__dirname, '..', '..', '..')
const localResourcesRoot = path.resolve(__dirname, '..', 'resources')
const desktopAppId = 'com.wunder.desktop'
const closePreferenceFileName = 'window-close-preference.json'
const companionLibraryStateFileName = 'desktop-companion-library-state.json'
const companionPackageStateFileName = 'desktop-companion-package-state.json'
const companionStateFileName = 'desktop-companion-state.json'
const closeBehaviorValues = new Set(['ask', 'tray', 'quit'])

const supportsLaunchAtLogin = () => process.platform === 'win32' || process.platform === 'darwin'

const resolveLaunchAtLoginArgs = () =>
  process.argv
    .slice(1)
    .map((item) => String(item || '').trim())
    .filter(Boolean)

const resolveLaunchAtLoginQueryOptions = () => {
  if (!supportsLaunchAtLogin() || app.isPackaged) {
    return undefined
  }
  return {
    path: process.execPath,
    args: resolveLaunchAtLoginArgs()
  }
}

const resolveLaunchAtLoginMutationOptions = (enabled) => {
  if (!supportsLaunchAtLogin()) {
    return null
  }
  const options = {
    openAtLogin: enabled === true
  }
  if (app.isPackaged) {
    return options
  }
  return {
    ...options,
    path: process.execPath,
    args: resolveLaunchAtLoginArgs()
  }
}

const prependProcessPathEntries = (entries) => {
  const existing = String(process.env.PATH || '')
    .split(path.delimiter)
    .map((item) => item.trim())
    .filter((item) => item)
  const merged = Array.from(new Set([...entries, ...existing]))
  process.env.PATH = merged.join(path.delimiter)
}

const normalizeProcessPathEntry = (entry) => {
  const raw = String(entry || '').trim()
  if (!raw) {
    return ''
  }
  const normalized = path.normalize(raw)
  return process.platform === 'win32' ? normalized.toLowerCase() : normalized
}

const removeProcessPathEntries = (entries) => {
  const blocked = new Set(entries.map((entry) => normalizeProcessPathEntry(entry)).filter((entry) => entry))
  if (!blocked.size) {
    return
  }
  const filtered = String(process.env.PATH || '')
    .split(path.delimiter)
    .map((item) => item.trim())
    .filter((item) => item && !blocked.has(normalizeProcessPathEntry(item)))
  process.env.PATH = filtered.join(path.delimiter)
}

const resolveDesktopAppDir = () => {
  const manual = String(process.env.WUNDER_DESKTOP_APP_DIR || '').trim()
  if (manual && fs.existsSync(manual)) {
    return manual
  }
  if (app.isPackaged && process.resourcesPath) {
    return path.dirname(process.resourcesPath)
  }
  return repoRoot
}

const resolveDesktopSettingsPath = () => {
  const manual = String(process.env.WUNDER_DESKTOP_SETTINGS_PATH || '').trim()
  if (manual) {
    return manual
  }
  return path.join(app.getPath('userData'), 'WUNDER_TEMPD', 'config', 'desktop.settings.json')
}

const resolveDesktopRuntimeRoot = () => path.join(app.getPath('userData'), 'WUNDER_RUNTIME')

const resolveCompanionStatePath = () => path.join(app.getPath('userData'), companionStateFileName)
const resolveCompanionPackageStatePath = () =>
  path.join(app.getPath('userData'), companionPackageStateFileName)
const resolveCompanionLibraryStatePath = () =>
  path.join(app.getPath('userData'), companionLibraryStateFileName)

const readJsonFile = (filePath, fallback = {}) => {
  try {
    if (!filePath || !fs.existsSync(filePath)) {
      return fallback
    }
    const raw = fs.readFileSync(filePath, 'utf8')
    if (!raw.trim()) {
      return fallback
    }
    const parsed = JSON.parse(raw)
    return parsed && typeof parsed === 'object' ? parsed : fallback
  } catch {
    return fallback
  }
}

const writeJsonFile = (filePath, value) => {
  try {
    fs.mkdirSync(path.dirname(filePath), { recursive: true })
    const tempPath = `${filePath}.${process.pid}.${Date.now()}.tmp`
    fs.writeFileSync(tempPath, `${JSON.stringify(value, null, 2)}\n`, 'utf8')
    fs.renameSync(tempPath, filePath)
  } catch {
    // Ignore desktop preference persistence failures.
  }
}

const writeDesktopSettings = (value) => {
  writeJsonFile(resolveDesktopSettingsPath(), value)
}

const resolveExistingFilePath = (value, appDir) => {
  const raw = String(value || '').trim()
  if (!raw) {
    return ''
  }
  const candidate = path.isAbsolute(raw) ? raw : path.join(appDir, raw)
  try {
    return fs.statSync(candidate).isFile() ? candidate : ''
  } catch {
    return ''
  }
}

const resolveToolFile = (value, appDir) => resolveExistingFilePath(value, appDir)

const readDesktopSettings = () => {
  const settingsPath = resolveDesktopSettingsPath()
  if (!settingsPath || !fs.existsSync(settingsPath)) {
    return {}
  }
  try {
    const raw = fs.readFileSync(settingsPath, 'utf8')
    if (!raw.trim()) {
      return {}
    }
    const parsed = JSON.parse(raw)
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

const resolveBundledPipBin = (appDir) => {
  const runtimeRoot = resolveDesktopRuntimeRoot()
  const candidates = process.platform === 'win32'
    ? [
        path.join(runtimeRoot, 'opt', 'python', 'Scripts', 'pip.exe'),
        path.join(runtimeRoot, 'opt', 'python', 'pip.exe'),
        path.join(runtimeRoot, 'opt', 'python', 'Scripts', 'pip3.exe'),
        path.join(runtimeRoot, 'opt', 'venv', 'Scripts', 'pip.exe'),
        path.join(appDir, 'opt', 'python', 'Scripts', 'pip.exe'),
        path.join(appDir, 'opt', 'python', 'pip.exe'),
        path.join(appDir, 'opt', 'python', 'Scripts', 'pip3.exe'),
        path.join(appDir, 'opt', 'venv', 'Scripts', 'pip.exe')
      ]
    : [
        path.join(runtimeRoot, 'opt', 'python', 'bin', 'pip3'),
        path.join(runtimeRoot, 'opt', 'python', 'bin', 'pip'),
        path.join(runtimeRoot, 'opt', 'venv', 'bin', 'pip3'),
        path.join(runtimeRoot, 'opt', 'venv', 'bin', 'pip'),
        path.join(appDir, 'opt', 'python', 'bin', 'pip3'),
        path.join(appDir, 'opt', 'python', 'bin', 'pip'),
        path.join(appDir, 'opt', 'venv', 'bin', 'pip3'),
        path.join(appDir, 'opt', 'venv', 'bin', 'pip')
      ]
  return candidates.find((candidate) => fs.existsSync(candidate)) || ''
}

const resolveBundledGitBin = (appDir) => {
  const runtimeRoot = resolveDesktopRuntimeRoot()
  const candidates = process.platform === 'win32'
    ? [
        path.join(runtimeRoot, 'opt', 'git', 'cmd', 'git.exe'),
        path.join(runtimeRoot, 'opt', 'git', 'bin', 'git.exe'),
        path.join(appDir, 'opt', 'git', 'cmd', 'git.exe'),
        path.join(appDir, 'opt', 'git', 'bin', 'git.exe')
      ]
    : [
        path.join(runtimeRoot, 'opt', 'git', 'bin', 'git'),
        path.join(runtimeRoot, 'opt', 'git', 'cmd', 'git'),
        path.join(appDir, 'opt', 'git', 'bin', 'git'),
        path.join(appDir, 'opt', 'git', 'cmd', 'git')
      ]
  return candidates.find((candidate) => fs.existsSync(candidate)) || ''
}

const normalizePythonRuntimeMode = (value, pythonPath = '') => {
  const rawPath = String(pythonPath || '').trim()
  if (rawPath) {
    return 'custom'
  }
  const mode = String(value || '').trim().toLowerCase()
  return mode === 'system' ? 'system' : 'auto'
}

const resolveBundledPythonBin = (appDir) => {
  const runtimeRoot = resolveDesktopRuntimeRoot()
  const candidates = process.platform === 'win32'
    ? [
        path.join(runtimeRoot, 'opt', 'python', 'python.exe'),
        path.join(runtimeRoot, 'opt', 'python', 'python3.exe'),
        path.join(runtimeRoot, 'opt', 'python', 'bin', 'python.exe'),
        path.join(runtimeRoot, 'opt', 'python', 'bin', 'python3.exe'),
        path.join(appDir, 'opt', 'python', 'python.exe'),
        path.join(appDir, 'opt', 'python', 'python3.exe'),
        path.join(appDir, 'opt', 'python', 'bin', 'python.exe'),
        path.join(appDir, 'opt', 'python', 'bin', 'python3.exe')
      ]
    : [
        path.join(runtimeRoot, 'opt', 'python', 'bin', 'python3'),
        path.join(runtimeRoot, 'opt', 'python', 'bin', 'python'),
        path.join(appDir, 'opt', 'python', 'bin', 'python3'),
        path.join(appDir, 'opt', 'python', 'bin', 'python')
      ]
  return candidates.find((candidate) => fs.existsSync(candidate)) || ''
}

const resolveBundledPythonDefaultBin = (appDir) =>
  process.platform === 'win32'
    ? path.join(resolveDesktopRuntimeRoot(), 'opt', 'python', 'python.exe')
    : path.join(resolveDesktopRuntimeRoot(), 'opt', 'python', 'bin', 'python3')

const resolveBundledVenvPythonBin = (appDir) => {
  const runtimeRoot = resolveDesktopRuntimeRoot()
  const candidates = process.platform === 'win32'
    ? [
        path.join(runtimeRoot, 'opt', 'venv', 'Scripts', 'python.exe'),
        path.join(runtimeRoot, 'opt', 'venv', 'python.exe'),
        path.join(appDir, 'opt', 'venv', 'Scripts', 'python.exe'),
        path.join(appDir, 'opt', 'venv', 'python.exe')
      ]
    : [
        path.join(runtimeRoot, 'opt', 'venv', 'bin', 'python3'),
        path.join(runtimeRoot, 'opt', 'venv', 'bin', 'python'),
        path.join(appDir, 'opt', 'venv', 'bin', 'python3'),
        path.join(appDir, 'opt', 'venv', 'bin', 'python')
      ]
  return candidates.find((candidate) => fs.existsSync(candidate)) || ''
}

const resolveBundledRgBin = (appDir) => {
  const runtimeRoot = resolveDesktopRuntimeRoot()
  const candidates = process.platform === 'win32'
    ? [
        path.join(runtimeRoot, 'opt', 'rg', 'rg.exe'),
        path.join(runtimeRoot, 'opt', 'rg', 'bin', 'rg.exe'),
        path.join(runtimeRoot, 'opt', 'ripgrep', 'rg.exe'),
        path.join(runtimeRoot, 'opt', 'ripgrep', 'bin', 'rg.exe'),
        path.join(appDir, 'opt', 'rg', 'rg.exe'),
        path.join(appDir, 'opt', 'rg', 'bin', 'rg.exe'),
        path.join(appDir, 'opt', 'ripgrep', 'rg.exe'),
        path.join(appDir, 'opt', 'ripgrep', 'bin', 'rg.exe')
      ]
    : [
        path.join(runtimeRoot, 'opt', 'rg', 'bin', 'rg'),
        path.join(runtimeRoot, 'opt', 'rg', 'rg'),
        path.join(runtimeRoot, 'opt', 'ripgrep', 'bin', 'rg'),
        path.join(runtimeRoot, 'opt', 'ripgrep', 'rg'),
        path.join(appDir, 'opt', 'rg', 'bin', 'rg'),
        path.join(appDir, 'opt', 'rg', 'rg'),
        path.join(appDir, 'opt', 'ripgrep', 'bin', 'rg'),
        path.join(appDir, 'opt', 'ripgrep', 'rg')
      ]
  return candidates.find((candidate) => fs.existsSync(candidate)) || ''
}

const collectDetectedPythonBins = (preferred = []) => {
  const output = []
  const seen = new Set()
  const pushCandidate = (value) => {
    const candidate = String(value || '').trim()
    if (!candidate || seen.has(candidate)) {
      return
    }
    try {
      if (!fs.statSync(candidate).isFile()) {
        return
      }
    } catch {
      return
    }
    seen.add(candidate)
    output.push(candidate)
  }
  preferred.forEach((item) => pushCandidate(item))

  const probeCommands = process.platform === 'win32'
    ? [['where.exe', ['python']], ['where.exe', ['python3']]]
    : [['which', ['python3']], ['which', ['python']]]
  for (const [command, args] of probeCommands) {
    try {
      const result = spawnSync(command, args, {
        encoding: 'utf8',
        windowsHide: true,
        timeout: 2000
      })
      if (result.error || result.status !== 0) {
        continue
      }
      const lines = `${String(result.stdout || '')}\n${String(result.stderr || '')}`
        .split(/\r?\n/)
        .map((item) => item.trim())
        .filter((item) => item)
      lines.forEach((item) => pushCandidate(item))
    } catch {
      // Keep fallback-only behavior when command probing is unavailable.
    }
  }
  return output
}

const resolveDesktopPythonRuntimeInfo = () => {
  const appDir = resolveDesktopAppDir()
  const settings = readDesktopSettings()
  const settingsBin = resolveExistingFilePath(settings.python_path, appDir)
  const settingsPipBin = resolveToolFile(settings.pip_path, appDir)
  const settingsGitBin = resolveToolFile(settings.git_path, appDir)
  const settingsRgBin = resolveToolFile(settings.rg_path, appDir)
  const envBin = resolveExistingFilePath(process.env.WUNDER_PYTHON_BIN, appDir)
  const runtimeMode = normalizePythonRuntimeMode(settings.python_runtime_mode, settings.python_path)
  const bundledDefaultBin = resolveBundledPythonDefaultBin(appDir)
  const bundledDefaultExists = fs.existsSync(bundledDefaultBin)
  const bundledBin = resolveBundledPythonBin(appDir)
  const bundledPipBin = resolveBundledPipBin(appDir)
  const bundledGitBin = resolveBundledGitBin(appDir)
  const bundledRgBin = resolveBundledRgBin(appDir)
  const venvBin = resolveBundledVenvPythonBin(appDir)
  const preferredBin =
    runtimeMode === 'system'
      ? ''
      : settingsBin || envBin || bundledBin || venvBin
  const normalizedBin = resolveExistingFilePath(preferredBin, appDir)
  const detectedBins = collectDetectedPythonBins([
    ...(runtimeMode === 'system' ? [] : [settingsBin, envBin, bundledBin, venvBin]),
    normalizedBin
  ])
  const normalizedAppDir = String(appDir || '')
    .trim()
    .replace(/\\/g, '/')
    .toLowerCase()
  const normalizedRuntimeBin = normalizedBin.replace(/\\/g, '/').toLowerCase()
  const bundledRoot = normalizedAppDir ? `${normalizedAppDir}/opt/python` : ''
  const venvRoot = normalizedAppDir ? `${normalizedAppDir}/opt/venv` : ''
  const bundled = Boolean(normalizedRuntimeBin && bundledRoot && normalizedRuntimeBin.startsWith(bundledRoot))
  const venv = Boolean(normalizedRuntimeBin && venvRoot && normalizedRuntimeBin.startsWith(venvRoot))
  const source = settingsBin
    ? 'settings'
    : bundled
      ? 'bundled'
      : venv
        ? 'venv'
        : envBin
          ? 'env'
          : normalizedBin
            ? 'path'
            : 'none'

  let version = ''
  if (normalizedBin) {
    try {
      const result = spawnSync(normalizedBin, ['--version'], {
        encoding: 'utf8',
        windowsHide: true,
        timeout: 2000
      })
      const raw = `${String(result.stdout || '')}\n${String(result.stderr || '')}`.trim()
      if (raw) {
        const firstLine = raw
          .split(/\r?\n/)
          .map((item) => item.trim())
          .find((item) => item.length > 0)
        version = firstLine || ''
      }
    } catch {
      version = ''
    }
  }

  return {
    bin: normalizedBin,
    version,
    source,
    bundled,
    runtime_mode: runtimeMode,
    bundled_default_bin: bundledDefaultBin,
    bundled_default_exists: bundledDefaultExists,
    detected_bins: detectedBins,
    pip_bin: settingsPipBin || bundledPipBin,
    git_bin: settingsGitBin || bundledGitBin,
    rg_bin: settingsRgBin || bundledRgBin,
    bundled_pip_bin: bundledPipBin,
    bundled_git_bin: bundledGitBin,
    bundled_rg_bin: bundledRgBin
  }
}

const buildSupplementTempPath = (prefix) =>
  fs.mkdtempSync(path.join(app.getPath('temp'), `${prefix}-`))

const removePathIfExists = (targetPath) => {
  if (!targetPath || !fs.existsSync(targetPath)) {
    return
  }
  fs.rmSync(targetPath, { recursive: true, force: true })
}

const copyDirectoryRecursive = (sourceDir, targetDir) => {
  const stat = fs.statSync(sourceDir)
  if (!stat.isDirectory()) {
    throw new Error(`Supplement source is not a directory: ${sourceDir}`)
  }
  fs.mkdirSync(targetDir, { recursive: true })
  const entries = fs.readdirSync(sourceDir, { withFileTypes: true })
  for (const entry of entries) {
    const sourcePath = path.join(sourceDir, entry.name)
    const targetPath = path.join(targetDir, entry.name)
    if (entry.isDirectory()) {
      copyDirectoryRecursive(sourcePath, targetPath)
      continue
    }
    fs.mkdirSync(path.dirname(targetPath), { recursive: true })
    fs.copyFileSync(sourcePath, targetPath)
  }
}

const extractZipArchiveWithPowershell = (zipPath, destinationDir) => {
  const scriptPath = path.join(
    app.getPath('temp'),
    `wunder-supplement-extract-${Date.now()}-${process.pid}.ps1`
  )
  const script = [
    'param([string]$ZipPath, [string]$Destination)',
    "$ErrorActionPreference = 'Stop'",
    'Add-Type -AssemblyName System.IO.Compression.FileSystem',
    'if (Test-Path -LiteralPath $Destination) {',
    '  Remove-Item -LiteralPath $Destination -Recurse -Force',
    '}',
    'New-Item -ItemType Directory -Path $Destination -Force | Out-Null',
    '[System.IO.Compression.ZipFile]::ExtractToDirectory($ZipPath, $Destination)'
  ].join('\r\n')
  fs.writeFileSync(scriptPath, script, 'utf8')
  try {
    const result = spawnSync(
      'powershell.exe',
      [
        '-NoLogo',
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        scriptPath,
        zipPath,
        destinationDir
      ],
      {
        encoding: 'utf8',
        windowsHide: true,
        timeout: 180000
      }
    )
    if (result.error) {
      throw result.error
    }
    if (result.status !== 0) {
      const detail = `${String(result.stdout || '')}\n${String(result.stderr || '')}`.trim()
      throw new Error(detail || `zip extraction failed with exit code ${result.status}`)
    }
  } finally {
    removePathIfExists(scriptPath)
  }
}

const hasSupplementContent = (rootDir) =>
  ['opt/python', 'opt/git', 'opt/rg'].some((relativePath) =>
    fs.existsSync(path.join(rootDir, ...relativePath.split('/')))
  )

const resolveImportedRuntimePaths = (installRoot) => {
  const pythonCandidates = process.platform === 'win32'
    ? [
        path.join(installRoot, 'opt', 'python', 'python.exe'),
        path.join(installRoot, 'opt', 'python', 'python3.exe'),
        path.join(installRoot, 'opt', 'python', 'bin', 'python.exe'),
        path.join(installRoot, 'opt', 'python', 'bin', 'python3.exe')
      ]
    : [
        path.join(installRoot, 'opt', 'python', 'bin', 'python3'),
        path.join(installRoot, 'opt', 'python', 'bin', 'python')
      ]
  const pipCandidates = process.platform === 'win32'
    ? [
        path.join(installRoot, 'opt', 'python', 'Scripts', 'pip.exe'),
        path.join(installRoot, 'opt', 'python', 'pip.exe'),
        path.join(installRoot, 'opt', 'python', 'Scripts', 'pip3.exe')
      ]
    : [
        path.join(installRoot, 'opt', 'python', 'bin', 'pip3'),
        path.join(installRoot, 'opt', 'python', 'bin', 'pip')
      ]
  const gitCandidates = process.platform === 'win32'
    ? [
        path.join(installRoot, 'opt', 'git', 'cmd', 'git.exe'),
        path.join(installRoot, 'opt', 'git', 'bin', 'git.exe')
      ]
    : [
        path.join(installRoot, 'opt', 'git', 'bin', 'git'),
        path.join(installRoot, 'opt', 'git', 'cmd', 'git')
      ]
  const rgCandidates = process.platform === 'win32'
    ? [
        path.join(installRoot, 'opt', 'rg', 'rg.exe'),
        path.join(installRoot, 'opt', 'rg', 'bin', 'rg.exe')
      ]
    : [
        path.join(installRoot, 'opt', 'rg', 'bin', 'rg'),
        path.join(installRoot, 'opt', 'rg', 'rg')
      ]
  const existing = (items) => items.find((candidate) => fs.existsSync(candidate)) || ''
  const pythonPath = existing(pythonCandidates)
  let pipPath = existing(pipCandidates)
  if (process.platform === 'win32' && pythonPath && !pipPath) {
    const wrapperPath = path.join(path.dirname(pythonPath), 'pip.cmd')
    try {
      fs.writeFileSync(wrapperPath, '@echo off\r\n"%~dp0python.exe" -m pip %*\r\n', 'utf8')
      pipPath = fs.existsSync(wrapperPath) ? wrapperPath : ''
    } catch {
      pipPath = ''
    }
  }
  return {
    python_path: pythonPath,
    pip_path: pipPath,
    git_path: existing(gitCandidates),
    rg_path: existing(rgCandidates)
  }
}

const resolveSupplementExtractRoot = (stagingDir) => {
  if (hasSupplementContent(stagingDir)) {
    return stagingDir
  }
  const entries = fs.readdirSync(stagingDir, { withFileTypes: true })
  for (const entry of entries) {
    if (!entry.isDirectory()) {
      continue
    }
    const candidate = path.join(stagingDir, entry.name)
    if (hasSupplementContent(candidate)) {
      return candidate
    }
  }
  return ''
}

const resolveDialogDefaultPath = (rawDefaultPath) => {
  const candidate = String(rawDefaultPath || '').trim()
  if (!candidate) {
    return ''
  }
  try {
    const stat = fs.statSync(candidate)
    if (stat.isDirectory()) {
      return candidate
    }
    if (stat.isFile()) {
      return path.dirname(candidate)
    }
  } catch {
    // Fallback to parent directory when input path does not exist.
  }
  const parent = path.dirname(candidate)
  return parent && parent !== '.' ? parent : ''
}

const choosePythonInterpreter = async (defaultPath = '') => {
  const normalizedDefaultPath = resolveDialogDefaultPath(defaultPath)
  const openDialogOptions = {
    title: 'Select Python Interpreter',
    defaultPath: normalizedDefaultPath || undefined,
    properties: ['openFile']
  }
  if (process.platform === 'win32') {
    openDialogOptions.filters = [{ name: 'Python Executable', extensions: ['exe'] }]
  }
  const result = await withMainWindow(
    (window) => dialog.showOpenDialog(window, openDialogOptions),
    () => dialog.showOpenDialog(openDialogOptions)
  )
  if (result?.canceled || !Array.isArray(result?.filePaths) || !result.filePaths.length) {
    return ''
  }
  return String(result.filePaths[0] || '').trim()
}

const chooseRuntimeExecutable = async (payload = {}) => {
  const defaultPath = typeof payload === 'string' ? payload : payload?.defaultPath
  const title = String(payload?.title || 'Select Runtime Executable')
  const normalizedDefaultPath = resolveDialogDefaultPath(defaultPath)
  const openDialogOptions = {
    title,
    defaultPath: normalizedDefaultPath || undefined,
    properties: ['openFile']
  }
  if (process.platform === 'win32') {
    openDialogOptions.filters = [{ name: 'Executable', extensions: ['exe', 'cmd', 'bat'] }]
  }
  const result = await withMainWindow(
    (window) => dialog.showOpenDialog(window, openDialogOptions),
    () => dialog.showOpenDialog(openDialogOptions)
  )
  if (result?.canceled || !Array.isArray(result?.filePaths) || !result.filePaths.length) {
    return ''
  }
  return String(result.filePaths[0] || '').trim()
}

const openPathWithDefaultApp = async (targetPath = '') => {
  const rawInput = String(targetPath || '').trim()
  if (!rawInput) {
    throw new Error('Path is required')
  }
  const normalized =
    process.platform === 'win32'
      ? path.win32.normalize(rawInput.replace(/\//g, '\\'))
      : path.normalize(rawInput)
  console.info('[desktop-debug][electron] openPathWithDefaultApp', {
    targetPath,
    normalized,
    exists: fs.existsSync(normalized)
  })
  if (!fs.existsSync(normalized)) {
    throw new Error(`Path not found: ${normalized}`)
  }
  const result = await shell.openPath(normalized)
  if (result) {
    console.error('[desktop-debug][electron] openPathWithDefaultApp failed', {
      normalized,
      result
    })
    throw new Error(result)
  }
  console.info('[desktop-debug][electron] openPathWithDefaultApp success', {
    normalized
  })
  return true
}

const importSupplementPackage = async () => {
  if (process.platform !== 'win32') {
    return {
      supported: false,
      canceled: false,
      installed: false
    }
  }
  const result = await withMainWindow(
    (window) =>
      dialog.showOpenDialog(window, {
        title: 'Import Wunder Supplement Package',
        filters: [{ name: 'Wunder Supplement Package', extensions: ['zip'] }],
        properties: ['openFile']
      }),
    () =>
      dialog.showOpenDialog({
        title: 'Import Wunder Supplement Package',
        filters: [{ name: 'Wunder Supplement Package', extensions: ['zip'] }],
        properties: ['openFile']
      })
  )
  if (result?.canceled || !Array.isArray(result?.filePaths) || !result.filePaths.length) {
    return {
      supported: true,
      canceled: true,
      installed: false
    }
  }

  const normalizedPackagePath = String(result.filePaths[0] || '').trim()
  if (!normalizedPackagePath) {
    return {
      supported: true,
      canceled: true,
      installed: false
    }
  }
  if (path.extname(normalizedPackagePath).toLowerCase() !== '.zip') {
    throw new Error(`Unsupported supplement package format: ${normalizedPackagePath}`)
  }
  const installRoot = resolveDesktopRuntimeRoot()
  const scriptPath = path.join(
    app.getPath('temp'),
    `wunder-supplement-import-${Date.now()}-${process.pid}.ps1`
  )
  const script = [
    'param([string]$ZipPath, [string]$InstallRoot, [string]$TempRoot)',
    "$ErrorActionPreference = 'Stop'",
    'function Emit-Json($obj) {',
    '  [Console]::Out.WriteLine(($obj | ConvertTo-Json -Compress -Depth 6))',
    '}',
    '$stage = Join-Path $TempRoot ("wunder-supplement-stage-" + [guid]::NewGuid().ToString("N"))',
    'try {',
    '  Emit-Json @{ type = "progress"; phase = "extracting"; progress = 14; summary = "正在解压补充包..." }',
    '  Add-Type -AssemblyName System.IO.Compression.FileSystem',
    '  if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }',
    '  New-Item -ItemType Directory -Path $stage -Force | Out-Null',
    '  [System.IO.Compression.ZipFile]::ExtractToDirectory($ZipPath, $stage)',
    '  Emit-Json @{ type = "progress"; phase = "verifying"; progress = 42; summary = "正在校验补充包内容..." }',
    '  $extractRoot = $stage',
    '  $targets = @("opt/python","opt/git","opt/rg")',
    '  $hasDirect = $false',
    '  foreach ($relative in $targets) { if (Test-Path -LiteralPath (Join-Path $extractRoot $relative)) { $hasDirect = $true; break } }',
    '  if (-not $hasDirect) {',
    '    $children = Get-ChildItem -LiteralPath $stage -Directory -ErrorAction SilentlyContinue',
    '    foreach ($child in $children) {',
    '      $candidate = $child.FullName',
    '      foreach ($relative in $targets) { if (Test-Path -LiteralPath (Join-Path $candidate $relative)) { $extractRoot = $candidate; $hasDirect = $true; break } }',
    '      if ($hasDirect) { break }',
    '    }',
    '  }',
    '  if (-not $hasDirect) { throw "Supplement package is missing opt/python, opt/git, and opt/rg" }',
    '  $imported = @()',
    '  for ($index = 0; $index -lt $targets.Count; $index++) {',
    '    $relative = $targets[$index]',
    '    $sourcePath = Join-Path $extractRoot $relative',
    '    if (-not (Test-Path -LiteralPath $sourcePath)) { continue }',
    '    Emit-Json @{ type = "progress"; phase = "copying"; progress = (56 + [math]::Round(($index / [math]::Max(1, $targets.Count)) * 28)); summary = ("正在导入 " + $relative + "...") }',
    '    $targetPath = Join-Path $InstallRoot $relative',
    '    if (Test-Path -LiteralPath $targetPath) { Remove-Item -LiteralPath $targetPath -Recurse -Force }',
    '    New-Item -ItemType Directory -Path (Split-Path -Parent $targetPath) -Force | Out-Null',
    '    Copy-Item -LiteralPath $sourcePath -Destination $targetPath -Recurse -Force',
    '    $imported += $targetPath',
    '  }',
    '  if ($imported.Count -eq 0) { throw "Supplement package does not contain importable runtime content" }',
    '  Emit-Json @{ type = "progress"; phase = "finalizing"; progress = 92; summary = "正在整理补充包元数据..." }',
    '  foreach ($fileName in @("README-win7-supplement.txt","wunder-win7-supplement.json")) {',
    '    $sourcePath = Join-Path $extractRoot $fileName',
    '    if (Test-Path -LiteralPath $sourcePath) { Copy-Item -LiteralPath $sourcePath -Destination (Join-Path $InstallRoot $fileName) -Force }',
    '  }',
    '  $pythonPath = ""',
    '  foreach ($relative in @("opt/python/python.exe","opt/python/python3.exe","opt/python/bin/python.exe","opt/python/bin/python3.exe")) { $candidate = Join-Path $InstallRoot $relative; if ((-not $pythonPath) -and (Test-Path -LiteralPath $candidate)) { $pythonPath = $candidate } }',
    '  $pipPath = ""',
    '  foreach ($relative in @("opt/python/Scripts/pip.exe","opt/python/pip.exe","opt/python/Scripts/pip3.exe")) { $candidate = Join-Path $InstallRoot $relative; if ((-not $pipPath) -and (Test-Path -LiteralPath $candidate)) { $pipPath = $candidate } }',
    '  if ((-not $pipPath) -and $pythonPath) {',
    '    $pipWrapper = Join-Path (Split-Path -Parent $pythonPath) "pip.cmd"',
    '    Set-Content -LiteralPath $pipWrapper -Encoding ASCII -Value "@echo off`r`n`"%~dp0python.exe`" -m pip %*`r`n"',
    '    if (Test-Path -LiteralPath $pipWrapper) { $pipPath = $pipWrapper }',
    '  }',
    '  $gitPath = ""',
    '  foreach ($relative in @("opt/git/cmd/git.exe","opt/git/bin/git.exe")) { $candidate = Join-Path $InstallRoot $relative; if ((-not $gitPath) -and (Test-Path -LiteralPath $candidate)) { $gitPath = $candidate } }',
    '  $rgPath = ""',
    '  foreach ($relative in @("opt/rg/rg.exe","opt/rg/bin/rg.exe")) { $candidate = Join-Path $InstallRoot $relative; if ((-not $rgPath) -and (Test-Path -LiteralPath $candidate)) { $rgPath = $candidate } }',
    '  Emit-Json @{ type = "result"; supported = $true; canceled = $false; installed = $true; install_root = $InstallRoot; package_path = $ZipPath; imported_paths = $imported; runtime_paths = @{ python_path = $pythonPath; pip_path = $pipPath; git_path = $gitPath; rg_path = $rgPath } }',
    '} catch {',
    '  Emit-Json @{ type = "error"; message = $_.Exception.Message }',
    '  exit 1',
    '} finally {',
    '  if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue }',
    '}'
  ].join('\r\n')
  fs.writeFileSync(scriptPath, script, 'utf8')
  return await new Promise((resolve, reject) => {
    const child = spawn('powershell.exe', [
      '-NoLogo',
      '-NoProfile',
      '-NonInteractive',
      '-ExecutionPolicy',
      'Bypass',
      '-File',
      scriptPath,
      normalizedPackagePath,
      installRoot,
      app.getPath('temp')
    ], {
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe']
    })
    let stderr = ''
    let settled = false

    child.stdout.on('data', (chunk) => {
      const text = String(chunk || '')
      text
        .split(/\r?\n/)
        .map((line) => line.trim())
        .filter(Boolean)
        .forEach((line) => {
          try {
            const payload = JSON.parse(line)
            if (payload?.type === 'result') {
              settled = true
              const runtimePaths = resolveImportedRuntimePaths(installRoot)
              const settings = readDesktopSettings()
              const nextSettings = {
                ...settings,
                ...Object.fromEntries(
                  Object.entries(runtimePaths).filter(([, value]) => String(value || '').trim())
                ),
                python_runtime_mode: 'custom',
                updated_at: Date.now() / 1000
              }
              writeDesktopSettings(nextSettings)
              payload.runtime_paths = {
                ...(payload.runtime_paths || {}),
                ...runtimePaths
              }
              registerBundledToolPaths()
              resolve(payload)
            } else if (payload?.type === 'error') {
              settled = true
              reject(new Error(String(payload.message || 'Supplement import failed')))
            }
          } catch {
            // Ignore malformed worker output.
          }
        })
    })

    child.stderr.on('data', (chunk) => {
      stderr += String(chunk || '')
    })

    child.on('error', (error) => {
      if (settled) return
      settled = true
      reject(error)
    })

    child.on('close', (code) => {
      if (settled) return
      settled = true
      reject(new Error(stderr.trim() || `Supplement import failed with exit code ${code}`))
    })
    child.on('exit', () => {
      removePathIfExists(scriptPath)
    })
  })
}

const registerBundledToolPaths = () => {
  const appDir = resolveDesktopAppDir()
  const runtimeRoot = resolveDesktopRuntimeRoot()
  const roots = [runtimeRoot, appDir].filter((candidate) => candidate && fs.existsSync(candidate))
  if (!roots.length) {
    return
  }

  process.env.WUNDER_DESKTOP_APP_DIR = appDir
  process.env.WUNDER_DESKTOP_RUNTIME_ROOT = runtimeRoot
  const settings = readDesktopSettings()
  const runtimeMode = normalizePythonRuntimeMode(settings.python_runtime_mode, settings.python_path)
  const settingsBin = resolveExistingFilePath(settings.python_path, appDir)
  const bundledPythonBin = resolveBundledPythonBin(appDir)
  if (runtimeMode === 'system') {
    delete process.env.WUNDER_PYTHON_BIN
  } else if (settingsBin) {
    process.env.WUNDER_PYTHON_BIN = settingsBin
  } else if (bundledPythonBin) {
    process.env.WUNDER_PYTHON_BIN = bundledPythonBin
  }
  const rgBin = resolveToolFile(settings.rg_path, appDir) || resolveBundledRgBin(appDir)
  if (rgBin) {
    process.env.WUNDER_RG_BIN = rgBin
  }

  const pythonCandidates = roots.flatMap((root) => [
    path.join(root, 'opt', 'python'),
    path.join(root, 'opt', 'python', 'Scripts'),
    path.join(root, 'opt', 'python', 'bin'),
    path.join(root, 'opt', 'venv'),
    path.join(root, 'opt', 'venv', 'Scripts'),
    path.join(root, 'opt', 'venv', 'bin')
  ])
  if (runtimeMode === 'system') {
    removeProcessPathEntries(pythonCandidates)
  }
  const candidates = [
    ...(runtimeMode === 'system' ? [] : pythonCandidates),
    ...roots.flatMap((root) => [
      path.join(root, 'opt', 'git', 'cmd'),
      path.join(root, 'opt', 'git', 'bin'),
      path.join(root, 'opt', 'rg'),
      path.join(root, 'opt', 'rg', 'bin'),
      path.join(root, 'opt', 'ripgrep'),
      path.join(root, 'opt', 'ripgrep', 'bin')
    ])
  ].filter((candidate) => fs.existsSync(candidate))

  if (!candidates.length) {
    return
  }

  // Keep bundled tool paths ahead of system PATH so a supplement package
  // extracted into the install directory is picked up automatically.
  prependProcessPathEntries(candidates)
}

const syncDesktopRuntimePathsFromSupplement = () => {
  if (process.platform !== 'win32') {
    return
  }
  const installRoot = resolveDesktopRuntimeRoot()
  if (!fs.existsSync(installRoot)) {
    return
  }
  const runtimePaths = resolveImportedRuntimePaths(installRoot)
  if (!Object.values(runtimePaths).some((value) => String(value || '').trim())) {
    return
  }
  const settings = readDesktopSettings()
  const missingRuntimePaths = Object.fromEntries(
    Object.entries(runtimePaths).filter(
      ([key, value]) => String(value || '').trim() && !String(settings[key] || '').trim()
    )
  )
  if (!Object.keys(missingRuntimePaths).length) {
    return
  }
  const nextSettings = {
    ...settings,
    ...missingRuntimePaths,
    python_runtime_mode:
      missingRuntimePaths.python_path ? 'custom' : settings.python_runtime_mode,
    updated_at: Date.now() / 1000
  }
  writeDesktopSettings(nextSettings)
}

syncDesktopRuntimePathsFromSupplement()
registerBundledToolPaths()

if (process.platform === 'win32') {
  app.setAppUserModelId(desktopAppId)
}

const getBridgeName = () => (process.platform === 'win32' ? 'wunder-desktop-bridge.exe' : 'wunder-desktop-bridge')

const resolveBridgePath = () => {
  if (process.env.WUNDER_BRIDGE_PATH) {
    return process.env.WUNDER_BRIDGE_PATH
  }
  const bridgeName = getBridgeName()
  if (app.isPackaged) {
    return path.join(process.resourcesPath, bridgeName)
  }
  const devCandidate = path.join(repoRoot, 'target', 'release', bridgeName)
  if (fs.existsSync(devCandidate)) {
    return devCandidate
  }
  return path.join(localResourcesRoot, bridgeName)
}

const resolveFrontendRoot = () => {
  if (process.env.WUNDER_FRONTEND_ROOT) {
    return process.env.WUNDER_FRONTEND_ROOT
  }
  if (app.isPackaged) {
    return path.join(process.resourcesPath, 'frontend-dist')
  }
  const devCandidate = path.join(repoRoot, 'frontend', 'dist')
  if (fs.existsSync(devCandidate)) {
    return devCandidate
  }
  return path.join(localResourcesRoot, 'frontend-dist')
}

const resolveBundledSkillsRoot = () => {
  const searchRoots = []
  if (app.isPackaged && process.resourcesPath) {
    searchRoots.push(process.resourcesPath)
  }
  searchRoots.push(localResourcesRoot)
  searchRoots.push(path.join(repoRoot, 'config'))
  for (const root of searchRoots) {
    const candidate = path.join(root, 'skills')
    if (fs.existsSync(candidate)) {
      return candidate
    }
  }
  return ''
}

const resolveWindowIcon = () => {
  const iconNames = process.platform === 'win32' ? ['icon.ico', 'icon.png'] : ['icon.png', 'icon.ico']
  const searchRoots = []
  if (app.isPackaged) {
    searchRoots.push(process.resourcesPath)
  }
  searchRoots.push(path.join(__dirname, '..', 'build'))
  searchRoots.push(path.join(__dirname, '..', 'assets'))
  for (const root of searchRoots) {
    for (const iconName of iconNames) {
      const iconPath = path.join(root, iconName)
      if (fs.existsSync(iconPath)) {
        return iconPath
      }
    }
  }
  return undefined
}

const resolveLinuxDesktopIconSource = () => {
  if (process.platform !== 'linux') {
    return null
  }
  const searchRoots = []
  if (app.isPackaged) {
    searchRoots.push(process.resourcesPath)
  }
  searchRoots.push(path.join(__dirname, '..', 'build'))
  for (const root of searchRoots) {
    const candidate = path.join(root, 'icon.png')
    if (fs.existsSync(candidate)) {
      return candidate
    }
  }
  return null
}

const showDesktopNotification = (payload) => {
  try {
    if (!Notification || !Notification.isSupported()) {
      return false
    }
    const title = String(payload?.title || '').trim()
    if (!title) {
      return false
    }
    const body = String(payload?.body || '').trim()
    const silent = payload?.silent === true
    const notification = new Notification({
      title,
      body: body || undefined,
      silent
    })
    notification.show()
    return true
  } catch (error) {
    return false
  }
}

const sanitizeCloseBehavior = (value) => {
  if (closeBehaviorValues.has(value)) {
    return value
  }
  return 'ask'
}

const resolveClosePreferencePath = () => path.join(app.getPath('userData'), closePreferenceFileName)

const loadCloseBehavior = () => {
  const preferencePath = resolveClosePreferencePath()
  if (!fs.existsSync(preferencePath)) {
    return 'ask'
  }
  try {
    const raw = fs.readFileSync(preferencePath, 'utf8')
    const parsed = JSON.parse(raw)
    return sanitizeCloseBehavior(parsed?.closeBehavior)
  } catch (error) {
    console.warn('Failed to read close preference:', error)
    return 'ask'
  }
}

const saveCloseBehavior = (value) => {
  const normalized = sanitizeCloseBehavior(value)
  const preferencePath = resolveClosePreferencePath()
  try {
    fs.mkdirSync(path.dirname(preferencePath), { recursive: true })
    fs.writeFileSync(preferencePath, JSON.stringify({ closeBehavior: normalized }, null, 2), 'utf8')
    closeBehavior = normalized
    return true
  } catch (error) {
    console.warn('Failed to persist close preference:', error)
    return false
  }
}

const getLaunchAtLoginState = () => {
  if (!supportsLaunchAtLogin()) {
    return {
      supported: false,
      enabled: false
    }
  }
  try {
    const settings = app.getLoginItemSettings(resolveLaunchAtLoginQueryOptions())
    return {
      supported: true,
      enabled: settings.openAtLogin === true
    }
  } catch (error) {
    console.warn('Failed to read launch-at-login state:', error)
    return {
      supported: false,
      enabled: false
    }
  }
}

const setLaunchAtLoginState = (value) => {
  const options = resolveLaunchAtLoginMutationOptions(value)
  if (!options) {
    return {
      supported: false,
      enabled: false
    }
  }
  app.setLoginItemSettings(options)
  return getLaunchAtLoginState()
}

const writeFileIfChanged = (filePath, content, mode) => {
  const previous = fs.existsSync(filePath) ? fs.readFileSync(filePath, 'utf8') : null
  if (previous === content) {
    return
  }
  fs.mkdirSync(path.dirname(filePath), { recursive: true })
  fs.writeFileSync(filePath, content, 'utf8')
  if (typeof mode === 'number') {
    fs.chmodSync(filePath, mode)
  }
}

const ensureLinuxDesktopIntegration = () => {
  if (process.platform !== 'linux') {
    return
  }
  const appImagePath = process.env.APPIMAGE || process.execPath
  if (!appImagePath || !fs.existsSync(appImagePath)) {
    return
  }

  const homeDir = app.getPath('home')
  const applicationsDir = path.join(homeDir, '.local', 'share', 'applications')
  const desktopFilePath = path.join(applicationsDir, 'wunder-desktop.desktop')

  let iconValue = 'wunder-desktop'
  const iconSource = resolveLinuxDesktopIconSource()
  if (iconSource) {
    const iconTargetDir = path.join(homeDir, '.local', 'share', 'icons', 'hicolor', '512x512', 'apps')
    const iconTarget = path.join(iconTargetDir, 'wunder-desktop.png')
    fs.mkdirSync(iconTargetDir, { recursive: true })
    fs.copyFileSync(iconSource, iconTarget)
    iconValue = iconTarget
  }

  const escapedExec = String(appImagePath).replace(/"/g, '\\"')
  const desktopEntry = [
    '[Desktop Entry]',
    'Version=1.0',
    'Type=Application',
    'Name=Wunder Desktop',
    'Comment=Wunder Desktop',
    `Exec="${escapedExec}" %U`,
    `Icon=${iconValue}`,
    'Terminal=false',
    'Categories=Utility;',
    'StartupWMClass=wunder-desktop',
    `X-AppImage-Version=${app.getVersion()}`,
    ''
  ].join('\n')

  writeFileIfChanged(desktopFilePath, desktopEntry, 0o644)

  const desktopDir = app.getPath('desktop')
  if (desktopDir && fs.existsSync(desktopDir)) {
    const desktopShortcutPath = path.join(desktopDir, 'Wunder Desktop.desktop')
    writeFileIfChanged(desktopShortcutPath, desktopEntry, 0o755)
  }

  try {
    const updater = spawn('update-desktop-database', [applicationsDir], {
      stdio: 'ignore',
      detached: true
    })
    updater.on('error', () => {})
    updater.unref()
  } catch {
    // ignore: not all distros provide update-desktop-database
  }
}

const scheduleLinuxDesktopIntegration = () => {
  if (process.platform !== 'linux' || linuxDesktopIntegrationScheduled) {
    return
  }
  linuxDesktopIntegrationScheduled = true
  setTimeout(() => {
    try {
      ensureLinuxDesktopIntegration()
    } catch (error) {
      console.warn('Failed to update Linux desktop integration:', error)
    }
  }, 120)
}

const normalizeUpdateMessage = (error) => {
  const text = String(error?.message || error || '').trim()
  if (!text) {
    return 'unknown update error'
  }
  if (/publish configuration/i.test(text) || /app-update\.yml/i.test(text)) {
    return 'update source is not configured'
  }
  return text
}

const setUpdateState = (patch) => {
  updateState = {
    ...updateState,
    ...patch,
    currentVersion: app.getVersion()
  }
}

const getUpdateState = () => ({
  ...updateState,
  currentVersion: app.getVersion()
})

const configureUpdaterEvents = () => {
  if (updaterReady) {
    return
  }
  if (updaterDisabledByBuild) {
    setUpdateState({
      phase: 'unsupported',
      message: runningInAppImage
        ? 'auto update is disabled for appimage package'
        : 'auto update is disabled by this build'
    })
    return
  }
  if (!autoUpdater) {
    setUpdateState({
      phase: 'unsupported',
      message: 'electron-updater is unavailable in this build'
    })
    return
  }
  updaterReady = true

  autoUpdater.autoDownload = false
  autoUpdater.autoInstallOnAppQuit = false
  autoUpdater.allowPrerelease = true
  autoUpdater.logger = console

  autoUpdater.on('checking-for-update', () => {
    setUpdateState({
      phase: 'checking',
      downloaded: false,
      progress: 0,
      message: ''
    })
  })

  autoUpdater.on('update-available', (info) => {
    setUpdateState({
      phase: 'available',
      latestVersion: String(info?.version || '').trim(),
      downloaded: false,
      progress: 0,
      message: ''
    })
  })

  autoUpdater.on('update-not-available', () => {
    setUpdateState({
      phase: 'not-available',
      latestVersion: '',
      downloaded: false,
      progress: 0,
      message: ''
    })
  })

  autoUpdater.on('download-progress', (progress) => {
    setUpdateState({
      phase: 'downloading',
      downloaded: false,
      progress: Number(progress?.percent || 0),
      message: ''
    })
  })

  autoUpdater.on('update-downloaded', (info) => {
    setUpdateState({
      phase: 'downloaded',
      latestVersion: String(info?.version || '').trim(),
      downloaded: true,
      progress: 100,
      message: ''
    })
  })

  autoUpdater.on('error', (error) => {
    setUpdateState({
      phase: 'error',
      message: normalizeUpdateMessage(error)
    })
  })
}

const checkAndDownloadUpdate = async () => {
  if (updateTask) {
    return updateTask
  }

  updateTask = (async () => {
    if (updaterDisabledByBuild) {
      setUpdateState({
        phase: 'unsupported',
        message: runningInAppImage
          ? 'auto update is disabled for appimage package'
          : 'auto update is disabled by this build'
      })
      return getUpdateState()
    }

    if (!app.isPackaged) {
      setUpdateState({
        phase: 'unsupported',
        message: 'auto update is only available in packaged app'
      })
      return getUpdateState()
    }

    if (!autoUpdater) {
      setUpdateState({
        phase: 'unsupported',
        message: 'electron-updater is unavailable in this build'
      })
      return getUpdateState()
    }

    configureUpdaterEvents()
    setUpdateState({
      phase: 'checking',
      latestVersion: '',
      downloaded: false,
      progress: 0,
      message: ''
    })

    try {
      const checkResult = await autoUpdater.checkForUpdates()
      if (!checkResult?.isUpdateAvailable || !checkResult?.updateInfo) {
        setUpdateState({
          phase: 'not-available',
          latestVersion: '',
          downloaded: false,
          progress: 0,
          message: ''
        })
        return getUpdateState()
      }

      const latestVersion = String(checkResult.updateInfo.version || '').trim()
      setUpdateState({
        phase: 'available',
        latestVersion,
        downloaded: false,
        progress: 0,
        message: ''
      })

      await autoUpdater.downloadUpdate()
      return getUpdateState()
    } catch (error) {
      setUpdateState({
        phase: 'error',
        message: normalizeUpdateMessage(error)
      })
      return getUpdateState()
    }
  })()

  try {
    return await updateTask
  } finally {
    updateTask = null
  }
}

const installDownloadedUpdate = () => {
  if (updaterDisabledByBuild) {
    return {
      ok: false,
      state: getUpdateState()
    }
  }
  if (!autoUpdater || !app.isPackaged || !updateState.downloaded || updateState.phase !== 'downloaded') {
    return {
      ok: false,
      state: getUpdateState()
    }
  }
  app.isQuitting = true
  setImmediate(() => {
    autoUpdater.quitAndInstall()
  })
  return {
    ok: true,
    state: getUpdateState()
  }
}

const getFreePort = () =>
  new Promise((resolve, reject) => {
    const server = net.createServer()
    server.unref()
    server.on('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const address = server.address()
      server.close(() => {
        if (address && typeof address === 'object') {
          resolve(address.port)
        } else {
          reject(new Error('Failed to resolve free port'))
        }
      })
    })
  })

const sleep = (ms) =>
  new Promise((resolve) => {
    setTimeout(resolve, Math.max(0, Number(ms) || 0))
  })

const buildScreenshotFileName = () => {
  const timestamp = new Date()
    .toISOString()
    .replace(/:/g, '-')
    .replace(/\./g, '-')
  return `screenshot-${timestamp}.png`
}

const appendScreenshotFileNameSuffix = (fileName, suffix) => {
  const normalized = String(fileName || '').trim()
  if (!normalized) return `screenshot${suffix}.png`
  const dotIndex = normalized.lastIndexOf('.')
  if (dotIndex <= 0) return `${normalized}${suffix}`
  return `${normalized.slice(0, dotIndex)}${suffix}${normalized.slice(dotIndex)}`
}

const resolveCaptureDisplay = (windowRef = null) => {
  const allDisplays = screen.getAllDisplays()
  if (!Array.isArray(allDisplays) || allDisplays.length === 0) {
    return screen.getPrimaryDisplay()
  }
  if (windowRef && !windowRef.isDestroyed()) {
    try {
      const windowBounds = windowRef.getBounds()
      const windowDisplay = screen.getDisplayMatching(windowBounds)
      if (windowDisplay) {
        return windowDisplay
      }
    } catch {
      // ignore window bounds errors
    }
  }
  try {
    const cursorPoint = screen.getCursorScreenPoint()
    const cursorDisplay = screen.getDisplayNearestPoint(cursorPoint)
    if (cursorDisplay) {
      return cursorDisplay
    }
  } catch {
    // ignore cursor position errors
  }
  return screen.getPrimaryDisplay() || allDisplays[0]
}

const resolveDisplayCaptureSize = (display, useScaleFactor = true) => {
  const scaleFactor = Math.max(1, Number(display?.scaleFactor || 1))
  const logicalWidth = Math.max(
    1,
    Number(display?.bounds?.width || display?.size?.width || 0)
  )
  const logicalHeight = Math.max(
    1,
    Number(display?.bounds?.height || display?.size?.height || 0)
  )
  const factor = useScaleFactor ? scaleFactor : 1
  const width = Math.max(1, Math.round(logicalWidth * factor))
  const height = Math.max(1, Math.round(logicalHeight * factor))
  return { width, height, scaleFactor }
}

const rankDisplaySources = (sources, expectedSize, targetDisplayId) => {
  const expectedWidth = Math.max(1, Number(expectedSize?.width || 1))
  const expectedHeight = Math.max(1, Number(expectedSize?.height || 1))
  const expectedArea = expectedWidth * expectedHeight
  const expectedAspect = expectedWidth / Math.max(1, expectedHeight)
  return sources
    .filter((item) => !item?.thumbnail?.isEmpty?.())
    .map((item) => {
      const imageSize = item.thumbnail.getSize()
      const imageWidth = Math.max(1, Number(imageSize?.width || 1))
      const imageHeight = Math.max(1, Number(imageSize?.height || 1))
      const imageArea = imageWidth * imageHeight
      const widthDiff = Math.abs(imageWidth - expectedWidth) / expectedWidth
      const heightDiff = Math.abs(imageHeight - expectedHeight) / expectedHeight
      const areaDiff = Math.abs(imageArea - expectedArea) / Math.max(1, expectedArea)
      const aspect = imageWidth / Math.max(1, imageHeight)
      const aspectDiff = Math.abs(aspect - expectedAspect) / Math.max(expectedAspect, 1e-6)
      let score = widthDiff + heightDiff + areaDiff + aspectDiff * 3
      if (targetDisplayId && String(item?.display_id || '') === targetDisplayId) {
        score -= 5
      }
      return { source: item, score }
    })
    .sort((left, right) => left.score - right.score)
}

const pickBestDisplaySource = (sources, expectedSize, targetDisplayId) => {
  if (!Array.isArray(sources) || sources.length === 0) {
    return null
  }
  const rankedSources = rankDisplaySources(sources, expectedSize, targetDisplayId)
  if (targetDisplayId) {
    const exactMatched = rankedSources.find(
      (item) => String(item?.source?.display_id || '') === targetDisplayId
    )
    if (exactMatched) {
      return exactMatched.source
    }
  }
  if (rankedSources.length > 0) {
    return rankedSources[0].source
  }
  return sources[0]
}

const resolveDisplaySourceWithSize = async (thumbnailSize, expectedSize, targetDisplayId) => {
  const sources = await desktopCapturer.getSources({
    types: ['screen'],
    thumbnailSize,
    fetchWindowIcons: false
  })
  return pickBestDisplaySource(sources, expectedSize, targetDisplayId)
}

const shouldRetryForSharpness = (actualSize, expectedSize) => {
  const actualWidth = Math.max(1, Number(actualSize?.width || 1))
  const actualHeight = Math.max(1, Number(actualSize?.height || 1))
  const expectedWidth = Math.max(1, Number(expectedSize?.width || 1))
  const expectedHeight = Math.max(1, Number(expectedSize?.height || 1))
  const actualArea = actualWidth * actualHeight
  const expectedArea = expectedWidth * expectedHeight
  if (!expectedArea) return false
  return actualArea / expectedArea < 0.85
}

const resolveDisplaySource = async (targetDisplay) => {
  const display = targetDisplay || screen.getPrimaryDisplay()
  const targetDisplayId = String(display?.id || '')
  const preferredSize = resolveDisplayCaptureSize(display, true)
  let bestSource = await resolveDisplaySourceWithSize(
    { width: preferredSize.width, height: preferredSize.height },
    preferredSize,
    targetDisplayId
  )

  if (!bestSource) {
    throw new Error('No screen source available')
  }

  let bestSize = bestSource.thumbnail.getSize()
  if (display?.scaleFactor > 1 && shouldRetryForSharpness(bestSize, preferredSize)) {
    const fallbackSize = resolveDisplayCaptureSize(display, false)
    if (
      fallbackSize.width !== preferredSize.width ||
      fallbackSize.height !== preferredSize.height
    ) {
      const fallbackSource = await resolveDisplaySourceWithSize(
        { width: fallbackSize.width, height: fallbackSize.height },
        preferredSize,
        targetDisplayId
      )
      if (fallbackSource && !fallbackSource.thumbnail.isEmpty()) {
        const fallbackSizeActual = fallbackSource.thumbnail.getSize()
        const bestArea = bestSize.width * bestSize.height
        const fallbackArea = fallbackSizeActual.width * fallbackSizeActual.height
        if (fallbackArea > bestArea) {
          bestSource = fallbackSource
          bestSize = fallbackSizeActual
        }
      }
    }
  }

  if (display?.scaleFactor > 1 && shouldRetryForSharpness(bestSize, preferredSize)) {
    try {
      const fullSizeSource = await resolveDisplaySourceWithSize(
        { width: 0, height: 0 },
        preferredSize,
        targetDisplayId
      )
      if (fullSizeSource && !fullSizeSource.thumbnail.isEmpty()) {
        const fullSize = fullSizeSource.thumbnail.getSize()
        const bestArea = bestSize.width * bestSize.height
        const fullArea = fullSize.width * fullSize.height
        if (fullArea > bestArea) {
          bestSource = fullSizeSource
        }
      }
    } catch {
      // ignore fallback failures
    }
  }

  return bestSource
}

const createScreenshotRegionSelectorHtml = (imageDataUrl) => `<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <title>Wunder Screenshot Selector</title>
  <style>
    html, body {
      margin: 0;
      width: 100%;
      height: 100%;
      overflow: hidden;
      background: #000;
      cursor: crosshair;
      user-select: none;
      font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
    }
    #screen {
      position: fixed;
      inset: 0;
      width: 100vw;
      height: 100vh;
      object-fit: contain;
      image-rendering: -webkit-optimize-contrast;
      image-rendering: crisp-edges;
      pointer-events: none;
    }
    #mask {
      position: fixed;
      inset: 0;
      background: rgba(2, 6, 23, 0.2);
      pointer-events: none;
    }
    #selection {
      position: fixed;
      border: 1px solid #f97316;
      background: rgba(249, 115, 22, 0.16);
      box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.25);
      display: none;
      pointer-events: none;
    }
    #hint {
      position: fixed;
      top: 14px;
      left: 50%;
      transform: translateX(-50%);
      display: inline-flex;
      align-items: center;
      gap: 12px;
      padding: 8px 12px;
      border-radius: 10px;
      border: 1px solid rgba(148, 163, 184, 0.36);
      background: rgba(15, 23, 42, 0.86);
      color: #e2e8f0;
      font-size: 13px;
      pointer-events: none;
      z-index: 20;
      max-width: calc(100vw - 24px);
      box-sizing: border-box;
    }
    #cancel {
      border: 1px solid rgba(148, 163, 184, 0.5);
      background: rgba(15, 23, 42, 0.9);
      color: #e2e8f0;
      border-radius: 8px;
      padding: 5px 10px;
      font-size: 12px;
      cursor: pointer;
      pointer-events: auto;
    }
  </style>
</head>
<body>
  <img id="screen" src="${imageDataUrl}" alt="" />
  <div id="mask"></div>
  <div id="selection"></div>
  <div id="hint">
    <span>拖拽选择截图区域，按 Esc 取消</span>
    <button id="cancel" type="button">取消</button>
  </div>
  <script>
    const { ipcRenderer } = require('electron');
    const screenImage = document.getElementById('screen');
    const selection = document.getElementById('selection');
    const hint = document.getElementById('hint');
    const cancelButton = document.getElementById('cancel');
    const clamp = (value, min, max) => Math.min(max, Math.max(min, value));
    const normalizeRect = (startX, startY, endX, endY) => {
      const left = Math.min(startX, endX);
      const top = Math.min(startY, endY);
      const width = Math.abs(endX - startX);
      const height = Math.abs(endY - startY);
      return { left, top, width, height };
    };
    const getImageRect = () => {
      const rect = screenImage.getBoundingClientRect();
      return {
        left: rect.left,
        top: rect.top,
        width: rect.width,
        height: rect.height,
        right: rect.right,
        bottom: rect.bottom
      };
    };
    const clampToImage = (x, y) => {
      const rect = getImageRect();
      const maxX = Math.max(rect.left, rect.right - 1);
      const maxY = Math.max(rect.top, rect.bottom - 1);
      return {
        x: clamp(x, rect.left, maxX),
        y: clamp(y, rect.top, maxY),
        rect
      };
    };

    let dragging = false;
    let startX = 0;
    let startY = 0;

    const updateSelection = (rect) => {
      selection.style.display = 'block';
      selection.style.left = rect.left + 'px';
      selection.style.top = rect.top + 'px';
      selection.style.width = rect.width + 'px';
      selection.style.height = rect.height + 'px';
    };

    const cancelSelection = () => {
      ipcRenderer.send('${SCREENSHOT_SELECTOR_CANCEL_CHANNEL}');
    };

    window.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        cancelSelection();
      }
    });

    cancelButton.addEventListener('click', (event) => {
      event.preventDefault();
      cancelSelection();
    });

    window.addEventListener('mousedown', (event) => {
      if (event.button !== 0) return;
      if (hint.contains(event.target)) return;
      const clamped = clampToImage(event.clientX, event.clientY);
      startX = clamped.x;
      startY = clamped.y;
      dragging = true;
      updateSelection({ left: startX, top: startY, width: 0, height: 0 });
    });

    window.addEventListener('mousemove', (event) => {
      if (!dragging) return;
      const clamped = clampToImage(event.clientX, event.clientY);
      const currentX = clamped.x;
      const currentY = clamped.y;
      updateSelection(normalizeRect(startX, startY, currentX, currentY));
    });

    window.addEventListener('mouseup', (event) => {
      if (!dragging) return;
      dragging = false;
      const clamped = clampToImage(event.clientX, event.clientY);
      const endX = clamped.x;
      const endY = clamped.y;
      const rect = normalizeRect(startX, startY, endX, endY);
      if (rect.width < 3 || rect.height < 3) {
        selection.style.display = 'none';
        return;
      }
      const imageRect = getImageRect();
      ipcRenderer.send('${SCREENSHOT_SELECTOR_RESULT_CHANNEL}', {
        x: rect.left,
        y: rect.top,
        width: rect.width,
        height: rect.height,
        imageLeft: imageRect.left,
        imageTop: imageRect.top,
        imageWidth: imageRect.width,
        imageHeight: imageRect.height,
        viewportWidth: window.innerWidth,
        viewportHeight: window.innerHeight
      });
    });
  </script>
</body>
</html>`

const pickScreenshotRegionFromBuffer = async (imageBuffer, targetDisplay) => {
  const sourceImage = nativeImage.createFromBuffer(imageBuffer)
  if (!sourceImage || sourceImage.isEmpty()) {
    return null
  }
  const imageSize = sourceImage.getSize()
  const display = targetDisplay || screen.getPrimaryDisplay()
  const bounds = display?.bounds || { x: 0, y: 0, width: 1280, height: 720 }
  const selectorWindow = new BrowserWindow({
    x: Number(bounds.x || 0),
    y: Number(bounds.y || 0),
    width: Math.max(1, Number(bounds.width || 1280)),
    height: Math.max(1, Number(bounds.height || 720)),
    frame: false,
    show: false,
    transparent: false,
    resizable: false,
    movable: false,
    minimizable: false,
    maximizable: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    fullscreen: false,
    fullscreenable: false,
    autoHideMenuBar: true,
    webPreferences: {
      nodeIntegration: true,
      contextIsolation: false,
      sandbox: false,
      devTools: false
    }
  })
  selectorWindow.setMenuBarVisibility(false)
  selectorWindow.setAlwaysOnTop(true, 'screen-saver')
  selectorWindow.setBounds({
    x: Number(bounds.x || 0),
    y: Number(bounds.y || 0),
    width: Math.max(1, Number(bounds.width || 1280)),
    height: Math.max(1, Number(bounds.height || 720))
  })

  const imageDataUrl = `data:image/png;base64,${imageBuffer.toString('base64')}`
  const html = createScreenshotRegionSelectorHtml(imageDataUrl)

  return new Promise((resolve) => {
    let settled = false
    const cleanup = () => {
      ipcMain.removeListener(SCREENSHOT_SELECTOR_RESULT_CHANNEL, handleRegionSelected)
      ipcMain.removeListener(SCREENSHOT_SELECTOR_CANCEL_CHANNEL, handleRegionCanceled)
      if (selectorWindow && !selectorWindow.isDestroyed()) {
        selectorWindow.destroy()
      }
    }
    const finish = (result) => {
      if (settled) return
      settled = true
      cleanup()
      resolve(result)
    }
    const handleRegionCanceled = (event) => {
      if (event?.sender?.id !== selectorWindow.webContents.id) return
      finish(null)
    }
    const handleRegionSelected = (event, payload) => {
      if (event?.sender?.id !== selectorWindow.webContents.id) return
      const imageLeft = Number(payload?.imageLeft || 0)
      const imageTop = Number(payload?.imageTop || 0)
      const imageViewportWidth = Math.max(
        1,
        Number(payload?.imageWidth || payload?.viewportWidth || 0)
      )
      const imageViewportHeight = Math.max(
        1,
        Number(payload?.imageHeight || payload?.viewportHeight || 0)
      )
      const relativeX = Math.max(0, Number(payload?.x || 0) - imageLeft)
      const relativeY = Math.max(0, Number(payload?.y || 0) - imageTop)
      const relativeWidth = Math.max(1, Number(payload?.width || 0))
      const relativeHeight = Math.max(1, Number(payload?.height || 0))
      const scaleX = imageSize.width / imageViewportWidth
      const scaleY = imageSize.height / imageViewportHeight
      const rawX = Math.max(0, Math.floor(relativeX * scaleX))
      const rawY = Math.max(0, Math.floor(relativeY * scaleY))
      const rawWidth = Math.max(1, Math.floor(relativeWidth * scaleX))
      const rawHeight = Math.max(1, Math.floor(relativeHeight * scaleY))
      const maxWidth = Math.max(1, imageSize.width - rawX)
      const maxHeight = Math.max(1, imageSize.height - rawY)
      const cropWidth = Math.min(rawWidth, maxWidth)
      const cropHeight = Math.min(rawHeight, maxHeight)
      if (cropWidth < 2 || cropHeight < 2) {
        finish(null)
        return
      }
      const cropped = sourceImage.crop({
        x: rawX,
        y: rawY,
        width: cropWidth,
        height: cropHeight
      })
      if (!cropped || cropped.isEmpty()) {
        finish(null)
        return
      }
      finish(cropped.toPNG())
    }

    ipcMain.on(SCREENSHOT_SELECTOR_RESULT_CHANNEL, handleRegionSelected)
    ipcMain.on(SCREENSHOT_SELECTOR_CANCEL_CHANNEL, handleRegionCanceled)
    selectorWindow.once('closed', () => finish(null))
    selectorWindow.webContents.on('did-fail-load', () => finish(null))
    selectorWindow.once('ready-to-show', () => {
      selectorWindow.show()
      selectorWindow.focus()
    })
    selectorWindow.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(html)}`).catch(() => finish(null))
  })
}

const captureDesktopScreenshot = async (options = {}) => {
  const hideWindow = options && options.hideWindow === true
  const region = options && options.region === true
  const window = mainWindow && !mainWindow.isDestroyed() ? mainWindow : null
  const wasVisible = Boolean(window?.isVisible?.())
  const captureDisplay = resolveCaptureDisplay(window)
  const shouldRestore = Boolean(window && hideWindow && wasVisible)

  try {
    if (shouldRestore && window) {
      window.hide()
      await sleep(SCREENSHOT_HIDE_DELAY_MS)
    }

    const source = await resolveDisplaySource(captureDisplay)
    if (!source || source.thumbnail.isEmpty()) {
      throw new Error('Failed to capture screenshot')
    }

    const imageBuffer = source.thumbnail.toPNG()
    if (!imageBuffer || imageBuffer.length === 0) {
      throw new Error('Screenshot image is empty')
    }

    let finalBuffer = imageBuffer
    let fileName = buildScreenshotFileName()
    if (region) {
      const regionBuffer = await pickScreenshotRegionFromBuffer(imageBuffer, captureDisplay)
      if (!regionBuffer || regionBuffer.length === 0) {
        return {
          ok: false,
          canceled: true,
          message: 'screenshot canceled'
        }
      }
      finalBuffer = regionBuffer
      fileName = appendScreenshotFileNameSuffix(fileName, '-region')
    }

    const screenshotDir = path.join(app.getPath('userData'), 'WUNDER_TEMPD', 'screenshots')
    const filePath = path.join(screenshotDir, fileName)
    fs.mkdirSync(screenshotDir, { recursive: true })
    fs.writeFileSync(filePath, finalBuffer)

    return {
      ok: true,
      name: fileName,
      path: filePath,
      mimeType: 'image/png',
      dataUrl: `data:image/png;base64,${finalBuffer.toString('base64')}`
    }
  } catch (error) {
    return {
      ok: false,
      message: String(error?.message || error || 'failed to capture screenshot')
    }
  } finally {
    if (
      shouldRestore &&
      window &&
      !window.isDestroyed() &&
      mainWindowVisibilityGuard.shouldRestoreAfterHiddenCapture({ window })
    ) {
      window.show()
      window.focus()
    }
  }
}

const waitForBridge = (resolvePort, timeoutMs = 15000) =>
  new Promise((resolve, reject) => {
    const startedAt = Date.now()
    let attempts = 0
    const attempt = () => {
      attempts += 1
      const port = resolvePort()
      if (!port) {
        retry()
        return
      }
      const socket = net.connect({ host: '127.0.0.1', port })
      socket.once('connect', () => {
        socket.destroy()
        resolve({ attempts })
      })
      socket.once('error', () => {
        socket.destroy()
        retry()
      })
      socket.setTimeout(700, () => {
        socket.destroy()
        retry()
      })
    }
    const retry = () => {
      if (Date.now() - startedAt > timeoutMs) {
        reject(new Error(`Bridge did not respond in time (attempts=${attempts})`))
        return
      }
      setTimeout(attempt, 80)
    }
    attempt()
  })

const isPortAvailable = (port) =>
  new Promise((resolve) => {
    const server = net.createServer()
    server.unref()
    server.on('error', () => resolve(false))
    server.listen(port, '127.0.0.1', () => {
      server.close(() => resolve(true))
    })
  })

const isLoopbackHostname = (host) => {
  const normalized = String(host || '').trim().toLowerCase()
  if (!normalized) {
    return false
  }
  return (
    normalized === 'localhost' ||
    normalized === '127.0.0.1' ||
    normalized === '::1' ||
    normalized === '[::1]' ||
    normalized.startsWith('127.')
  )
}

const normalizeOrigin = (originUrl) => {
  if (!originUrl) {
    return ''
  }
  try {
    const parsed = new URL(originUrl)
    if (!['http:', 'https:'].includes(parsed.protocol)) {
      return ''
    }
    return parsed.origin
  } catch {
    return ''
  }
}

const resolveMainWindowOrigin = () => {
  if (!mainWindow || mainWindow.isDestroyed() || mainWindow.webContents.isDestroyed()) {
    return ''
  }
  return normalizeOrigin(mainWindow.webContents.getURL())
}

const isSameOrigin = (left, right) => {
  const leftOrigin = normalizeOrigin(left)
  const rightOrigin = normalizeOrigin(right)
  if (!leftOrigin || !rightOrigin) {
    return false
  }
  return leftOrigin === rightOrigin
}

const resolveMediaPermissionOrigin = (webContents, requestingOrigin, details) => {
  const candidates = [
    details?.securityOrigin,
    details?.requestingUrl,
    requestingOrigin,
    webContents?.getURL?.(),
    resolveMainWindowOrigin(),
    bridgeWebBase,
    bridgePort ? `http://127.0.0.1:${bridgePort}` : ''
  ]
  for (const candidate of candidates) {
    const origin = normalizeOrigin(candidate)
    if (origin) {
      return origin
    }
  }
  return ''
}

const isTrustedMediaOrigin = (originUrl) => {
  const origin = normalizeOrigin(originUrl)
  if (!origin) {
    return false
  }
  const parsed = new URL(origin)
  if (isLoopbackHostname(parsed.hostname)) {
    return true
  }
  if (bridgeWebBase && isSameOrigin(bridgeWebBase, origin)) {
    return true
  }
  const mainOrigin = resolveMainWindowOrigin()
  if (mainOrigin && isSameOrigin(mainOrigin, origin)) {
    return true
  }
  return false
}

const configureMediaPermissions = () => {
  if (!session || !session.defaultSession) {
    return
  }
  const mediaPermissions = new Set([
    'media',
    'mediaAudioCapture',
    'mediaVideoCapture',
    'microphone',
    'camera'
  ])
  session.defaultSession.setPermissionRequestHandler((webContents, permission, callback, details) => {
    if (!mediaPermissions.has(permission)) {
      callback(false)
      return
    }
    const origin = resolveMediaPermissionOrigin(webContents, '', details)
    callback(isTrustedMediaOrigin(origin))
  })
  session.defaultSession.setPermissionCheckHandler((webContents, permission, requestingOrigin, details) => {
    if (!mediaPermissions.has(permission)) {
      return false
    }
    const origin = resolveMediaPermissionOrigin(webContents, requestingOrigin, details)
    return isTrustedMediaOrigin(origin)
  })
}

const normalizeMediaKind = (kind) => {
  const normalized = String(kind || '')
    .trim()
    .toLowerCase()
  if (normalized === 'microphone' || normalized === 'camera') {
    return normalized
  }
  return ''
}

const getMediaAccessStatus = (kind) => {
  const normalized = normalizeMediaKind(kind)
  if (!normalized || typeof systemPreferences?.getMediaAccessStatus !== 'function') {
    return 'unknown'
  }
  try {
    return String(systemPreferences.getMediaAccessStatus(normalized) || 'unknown')
  } catch {
    return 'unknown'
  }
}

const requestMediaAccess = async (kind) => {
  const normalized = normalizeMediaKind(kind)
  if (!normalized) {
    return false
  }
  const status = getMediaAccessStatus(normalized)
  if (status === 'granted') {
    return true
  }
  if (status === 'denied' || status === 'restricted') {
    return false
  }
  if (process.platform === 'darwin' && typeof systemPreferences?.askForMediaAccess === 'function') {
    try {
      return await systemPreferences.askForMediaAccess(normalized)
    } catch {
      return false
    }
  }
  return true
}

const normalizeCompanionState = (value) => {
  const source = value && typeof value === 'object' ? value : {}
  const enabled = source.enabled === true
  const key = String(source.key || source.id || '').trim()
  const agentId = String(source.agentId || source.agent_id || '').trim()
  const selectedId = String(source.selectedId || source.selected_id || '').trim()
  const displayName = String(source.displayName || source.display_name || '').trim()
  const description = String(source.description || '').trim()
  const spritesheetDataUrl = String(source.spritesheetDataUrl || source.spritesheet_data_url || '').trim()
  const state = normalizeCompanionBaseState(source.state)
  const scale = Number(source.scale)
  const x = Number(source.x)
  const y = Number(source.y)
  const message = String(source.message || '').trim()
  const messageKind = String(source.messageKind || source.message_kind || '').trim().toLowerCase()
  const messageVisible = source.messageVisible === true || source.message_visible === true
  return {
    enabled,
    key,
    agentId,
    selectedId,
    displayName,
    description,
    spritesheetDataUrl,
    state,
    scale: Number.isFinite(scale) ? Math.min(1.6, Math.max(0.7, scale)) : 1,
    x: Number.isFinite(x) ? Math.max(0, Math.round(x)) : 28,
    y: Number.isFinite(y) ? Math.max(0, Math.round(y)) : 28,
    message,
    messageKind: messageKind === 'success' || messageKind === 'warning' ? messageKind : 'info',
    messageVisible
  }
}

const serializeCompanionState = (value) => {
  const normalized = normalizeCompanionState(value)
  return {
    enabled: normalized.enabled,
    key: normalized.key,
    agentId: normalized.agentId,
    selectedId: normalized.selectedId,
    displayName: normalized.displayName,
    description: normalized.description,
    spritesheetDataUrl: normalized.spritesheetDataUrl,
    state: normalized.state,
    scale: normalized.scale,
    x: normalized.x,
    y: normalized.y
  }
}

const hasPersistableCompanionState = (value) => {
  const state = normalizeCompanionState(value)
  return Boolean(state.key || state.agentId || state.selectedId || state.spritesheetDataUrl)
}

const normalizePersistedCompanionStates = (value) => {
  const source = value && typeof value === 'object' ? value : {}
  const groups = []
  if (Array.isArray(source.runtimes)) {
    groups.push(source.runtimes)
  }
  if (Array.isArray(source.states)) {
    groups.push(source.states)
  }
  if (Array.isArray(source.companions)) {
    groups.push(source.companions)
  }
  const candidates = groups.flat()
  if (!candidates.length && hasPersistableCompanionState(source)) {
    candidates.push(source)
  }
  const map = new Map()
  candidates.forEach((item) => {
    const normalized = normalizeCompanionState({
      ...(item && typeof item === 'object' ? item : {}),
      message: '',
      messageVisible: false
    })
    if (!hasPersistableCompanionState(normalized)) {
      return
    }
    map.set(normalizeCompanionWindowKey(normalized), normalized)
  })
  return Array.from(map.values())
}

const collectCompanionPersistedStates = () => {
  const map = new Map()
  Array.from(companionRuntimes.values()).forEach((runtime) => {
    const state = normalizeCompanionState(runtime.state)
    if (state.enabled && hasPersistableCompanionState(state)) {
      map.set(normalizeCompanionWindowKey(state), serializeCompanionState(state))
    }
  })
  const primaryState = normalizeCompanionState(companionState)
  if (primaryState.enabled && hasPersistableCompanionState(primaryState)) {
    map.set(normalizeCompanionWindowKey(primaryState), serializeCompanionState(primaryState))
  }
  return Array.from(map.values())
}

const serializeCompanionStateFile = () => {
  const runtimes = collectCompanionPersistedStates()
  const currentActiveKey = normalizeCompanionWindowKey(companionState)
  const activeState = runtimes.find((state) => normalizeCompanionWindowKey(state) === currentActiveKey) || runtimes[0] || null
  return {
    version: 2,
    activeKey: activeState ? normalizeCompanionWindowKey(activeState) : '',
    runtimes,
    updated_at: Date.now()
  }
}

const loadCompanionState = () => {
  const persisted = readJsonFile(resolveCompanionStatePath(), {})
  const persistedStates = normalizePersistedCompanionStates(persisted)
  persistedStates.forEach((state) => {
    const runtime = getCompanionRuntime(state)
    runtime.state = normalizeCompanionState({
      ...runtime.state,
      ...state,
      message: '',
      messageVisible: false
    })
  })
  const activeKey = String(persisted.activeKey || persisted.active_key || '').trim()
  const activeState = activeKey
    ? persistedStates.find((state) => normalizeCompanionWindowKey(state) === activeKey)
    : null
  const activeRuntime =
    (activeState ? getCompanionRuntime(activeState) : null) ||
    (persistedStates[0] ? getCompanionRuntime(persistedStates[0]) : null)
  companionState = normalizeCompanionState({
    ...companionState,
    ...(activeRuntime?.state || persistedStates[0] || persisted),
    message: '',
    messageVisible: false
  })
  if (activeRuntime) {
    syncLegacyCompanionGlobals(activeRuntime)
  }
  return {
    ...companionState,
    runtimes: collectCompanionPersistedStates()
  }
}

const saveCompanionState = (patch) => {
  const source = patch && typeof patch === 'object' ? patch : {}
  const runtime = hasCompanionRuntimeIdentity(source)
    ? getCompanionRuntime(source)
    : findCompanionRuntime(companionState) || getCompanionRuntime(companionState)
  runtime.state = normalizeCompanionState({
    ...runtime.state,
    ...source
  })
  syncLegacyCompanionGlobals(runtime)
  writeJsonFile(resolveCompanionStatePath(), serializeCompanionStateFile())
  return runtime.state
}

const loadCompanionPackageState = () => readJsonFile(resolveCompanionPackageStatePath(), {})

const saveCompanionPackageState = (value) => {
  writeJsonFile(resolveCompanionPackageStatePath(), value && typeof value === 'object' ? value : {})
}

const loadCompanionLibraryState = () => readJsonFile(resolveCompanionLibraryStatePath(), {
  companions: [],
  settings: {},
  agentOverrides: {}
})

const normalizeCompanionLibraryRecord = (value) => {
  const source = value && typeof value === 'object' ? value : {}
  const id = String(source.id || source.selectedId || source.selected_id || '').trim()
  const displayName = String(
    source.companionDisplayName ||
    source.companion_display_name ||
    source.displayName ||
    source.display_name ||
    id
  ).trim()
  const spritesheetDataUrl = String(source.spritesheetDataUrl || source.spritesheet_data_url || '').trim()
  const spritesheetPath = String(source.spritesheetPath || source.spritesheet_path || '').trim()
  if (!id || !displayName || !spritesheetDataUrl.startsWith('data:image/')) {
    return null
  }
  const mimeMatch = /^data:([^;]+);/i.exec(spritesheetDataUrl)
  const spritesheetMime = String(
    source.spritesheetMime ||
    source.spritesheet_mime ||
    mimeMatch?.[1] ||
    'image/webp'
  ).trim()
  const extension = spritesheetMime.split('/').pop() || 'webp'
  const now = Date.now()
  return {
    id,
    displayName,
    description: String(source.description || '').trim(),
    spritesheetPath: spritesheetPath || `${id}.${extension}`,
    spritesheetDataUrl,
    spritesheetMime,
    importedAt: Number.isFinite(Number(source.importedAt || source.imported_at))
      ? Number(source.importedAt || source.imported_at)
      : now,
    updatedAt: Number.isFinite(Number(source.updatedAt || source.updated_at))
      ? Number(source.updatedAt || source.updated_at)
      : now,
    scope: 'private'
  }
}

const mergeCompanionLibraryRecords = (...groups) => {
  const map = new Map()
  groups.flat().forEach((item) => {
    const normalized = normalizeCompanionLibraryRecord(item)
    if (!normalized) {
      return
    }
    const current = map.get(normalized.id)
    if (!current || normalized.updatedAt >= current.updatedAt || (normalized.spritesheetDataUrl && !current.spritesheetDataUrl)) {
      map.set(normalized.id, normalized)
    }
  })
  return Array.from(map.values()).sort((a, b) => b.updatedAt - a.updatedAt)
}

const saveCompanionLibraryState = (value) => {
  const source = value && typeof value === 'object' ? value : {}
  writeJsonFile(resolveCompanionLibraryStatePath(), {
    companions: mergeCompanionLibraryRecords(Array.isArray(source.companions) ? source.companions : []),
    settings: source.settings && typeof source.settings === 'object' ? source.settings : {},
    agentOverrides:
      source.agentOverrides && typeof source.agentOverrides === 'object' ? source.agentOverrides : {},
    updated_at: Date.now()
  })
}

const parseBridgePort = (line) => {
  const trimmed = line.trim()
  const match = trimmed.match(/- web_base:\s*(https?:\/\/\S+)/)
  if (!match) {
    return null
  }
  try {
    const url = new URL(match[1])
    if (!url.port) {
      return null
    }
    // Keep renderer on a secure loopback origin to preserve mic/camera APIs.
    bridgeWebBase = isLoopbackHostname(url.hostname) ? url.origin : null
    return Number(url.port)
  } catch {
    return null
  }
}

const startBridge = async () => {
  const startBridgeNs = process.hrtime.bigint()
  const bridgePath = resolveBridgePath()
  if (!fs.existsSync(bridgePath)) {
    throw new Error(`Bridge binary not found: ${bridgePath}`)
  }

  const frontendRoot = resolveFrontendRoot()
  const hasFrontendRoot = Boolean(frontendRoot && fs.existsSync(frontendRoot))
  const tempRoot = path.join(app.getPath('userData'), 'WUNDER_TEMPD')
  const workspaceRoot = path.join(app.getPath('userData'), 'WUNDER_WORK')
  console.info('[desktop-debug][electron] bridge paths', {
    frontendRoot,
    hasFrontendRoot,
    tempRoot,
    workspaceRoot
  })
  bridgePort = await getFreePort()
  logStartupSegment('electron', 'bridge_prepare_paths', startBridgeNs, {
    has_frontend_root: hasFrontendRoot ? 1 : 0,
    port: bridgePort
  })

  const args = [
    '--host',
    '127.0.0.1',
    '--port',
    String(bridgePort),
    '--temp-root',
    tempRoot,
    '--workspace',
    workspaceRoot
  ]

  if (hasFrontendRoot && frontendRoot) {
    args.push('--frontend-root', frontendRoot)
  }

  const bridgeEnv = { ...process.env }
  const bundledSkillsRoot = resolveBundledSkillsRoot()
  if (bundledSkillsRoot && !bridgeEnv.WUNDER_BUILTIN_SKILLS_ROOT) {
    bridgeEnv.WUNDER_BUILTIN_SKILLS_ROOT = bundledSkillsRoot
  }

  const bridgeSpawnNs = process.hrtime.bigint()
  bridgeProcess = spawn(bridgePath, args, {
    env: bridgeEnv,
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true
  })

  logStartupSegment('electron', 'bridge_spawn_process', bridgeSpawnNs, {
    pid: bridgeProcess?.pid || 0,
    port: bridgePort
  })

  bridgeProcess.stdout.on('data', (data) => {
    const text = data.toString()
    const lines = text.split(/\r?\n/)
    for (const line of lines) {
      const trimmed = line.trim()
      if (!trimmed) {
        continue
      }
      const isStartupLine = trimmed.startsWith('[startup]')
      const isInfoLine = /\bINFO\b/.test(trimmed)
      if (bridgeVerboseLogs || (!isStartupLine && !isInfoLine)) {
        console.log(`[bridge] ${trimmed}`)
      }
      const parsedPort = parseBridgePort(trimmed)
      if (parsedPort) {
        bridgePort = parsedPort
      }
    }
  })
  bridgeProcess.stderr.on('data', (data) => {
    const text = data.toString().trim()
    if (text) {
      console.error(`[bridge] ${text}`)
    }
  })
  bridgeProcess.on('exit', (code, signal) => {
    if (bridgeRestarting) {
      return
    }
    if (app.isQuitting) {
      return
    }
    const message = `Bridge exited${code !== null ? ` (code ${code})` : ''}${
      signal ? ` with signal ${signal}` : ''
    }.`
    console.error(message)
    dialog.showErrorBox('Wunder Desktop', message)
    app.quit()
  })

  const bridgeWaitReadyNs = process.hrtime.bigint()
  const bridgeWaitResult = await waitForBridge(() => bridgePort)
  logStartupSegment('electron', 'bridge_wait_ready', bridgeWaitReadyNs, {
    attempts: bridgeWaitResult?.attempts || 0,
    port: bridgePort
  })
  logStartupSegment('electron', 'bridge_start_total', startBridgeNs, {
    port: bridgePort
  })
  return bridgePort
}

const stopBridge = () => {
  if (!bridgeProcess) {
    return
  }
  try {
    bridgeProcess.kill()
  } catch (err) {
    console.warn('Failed to stop bridge process:', err)
  }
  bridgeProcess = null
}

const waitForPortRelease = async (port, timeoutMs = 8000) => {
  const startedAt = Date.now()
  while (Date.now() - startedAt < timeoutMs) {
    const available = await isPortAvailable(port)
    if (available) {
      return true
    }
    await new Promise((resolve) => setTimeout(resolve, 120))
  }
  return false
}

const restartBridge = async () => {
  const previousQuitting = app.isQuitting === true
  bridgeRestarting = true
  const previousPort = bridgePort
  stopBridge()
  if (previousPort) {
    await waitForPortRelease(previousPort)
  }
  bridgePort = previousPort || null
  bridgeWebBase = null
  try {
    await startBridge()
    app.isQuitting = previousQuitting
    return true
  } finally {
    bridgeRestarting = false
  }
}

const toggleMainDevTools = () => {
  if (!mainWindow || mainWindow.isDestroyed()) {
    return false
  }
  const contents = mainWindow.webContents
  if (contents.isDevToolsOpened()) {
    contents.closeDevTools()
    return false
  }
  contents.openDevTools({ mode: 'detach' })
  return true
}

const withMainWindow = (handler, fallback) => {
  if (!mainWindow || mainWindow.isDestroyed()) {
    if (typeof fallback === 'function') {
      return fallback()
    }
    return fallback
  }
  return handler(mainWindow)
}

const showMainWindow = (options = {}) =>
  withMainWindow((window) => {
    if (mainWindowVisibilityGuard.isAutoShowBlocked({ explicit: options.explicit === true })) {
      return false
    }
    if (window.isMinimized()) {
      window.restore()
    }
    if (!window.isVisible()) {
      window.show()
    }
    window.focus()
    mainWindowVisibilityGuard.clearManualMinimize()
    // Force an immediate repaint after restore/show to avoid stale blank frames.
    if (!window.webContents.isDestroyed()) {
      window.webContents.invalidate()
      setTimeout(() => {
        if (!window.isDestroyed() && !window.webContents.isDestroyed()) {
          window.webContents.invalidate()
        }
      }, 80)
    }
    return true
  }, false)

const destroyTray = () => {
  if (!tray) {
    return
  }
  tray.destroy()
  tray = null
}

const createTray = () => {
  if (tray) {
    return tray
  }
  const iconPath = resolveWindowIcon()
  if (!iconPath) {
    console.warn('Tray icon missing, skip tray integration.')
    return null
  }
  const trayIcon = nativeImage.createFromPath(iconPath)
  if (trayIcon.isEmpty()) {
    console.warn(`Failed to load tray icon: ${iconPath}`)
    return null
  }
  tray = new Tray(trayIcon)
  tray.setToolTip('Wunder Desktop')
  const trayMenu = Menu.buildFromTemplate([
    {
      label: '打开 Wunder Desktop',
      click: () => {
        showMainWindow({ explicit: true })
      }
    },
    { type: 'separator' },
    {
      label: '退出',
      click: () => {
        app.isQuitting = true
        app.quit()
      }
    }
  ])
  tray.setContextMenu(trayMenu)
  tray.on('click', () => {
    showMainWindow()
  })
  tray.on('double-click', () => {
    showMainWindow({ explicit: true })
  })
  return tray
}

const hideMainWindowToTray = () =>
  withMainWindow((window) => {
    const trayInstance = createTray()
    if (!trayInstance) {
      app.isQuitting = true
      app.quit()
      return false
    }
    window.hide()
    return true
  }, false)

const promptCloseBehavior = async (window) => {
  if (closePromptInFlight) {
    return
  }
  closePromptInFlight = true
  try {
    const result = await dialog.showMessageBox(window, {
      type: 'question',
      title: 'Wunder Desktop',
      message: '关闭窗口时，您希望如何处理？',
      detail: '可选择隐藏到托盘继续后台运行，或直接退出程序。',
      buttons: ['隐藏到托盘', '关闭退出', '取消'],
      defaultId: 0,
      cancelId: 2,
      noLink: true,
      checkboxLabel: '下次不再提示',
      checkboxChecked: false
    })

    if (result.response === 0) {
      if (result.checkboxChecked) {
        saveCloseBehavior('tray')
      }
      hideMainWindowToTray()
      return
    }

    if (result.response === 1) {
      if (result.checkboxChecked) {
        saveCloseBehavior('quit')
      }
      app.isQuitting = true
      app.quit()
    }
  } finally {
    closePromptInFlight = false
  }
}

const handleMainWindowClose = (window, event) => {
  if (app.isQuitting) {
    return
  }
  const behavior = sanitizeCloseBehavior(closeBehavior)
  if (behavior === 'quit') {
    return
  }
  event.preventDefault()
  if (behavior === 'tray') {
    hideMainWindowToTray()
    return
  }
  void promptCloseBehavior(window)
}

const createLoadingHtml = () => `<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <meta http-equiv="x-ua-compatible" content="ie=edge" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Wunder Desktop</title>
  <style>
    :root { color-scheme: light dark; }
    body {
      margin: 0;
      font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
      display: flex;
      align-items: center;
      justify-content: center;
      height: 100vh;
      background: #0b1220;
      color: #e2e8f0;
    }
    .shell {
      text-align: center;
      max-width: 360px;
    }
    .title {
      font-size: 18px;
      font-weight: 600;
      margin-bottom: 12px;
    }
    .spinner {
      width: 32px;
      height: 32px;
      border-radius: 999px;
      border: 3px solid rgba(148, 163, 184, 0.3);
      border-top-color: #38bdf8;
      margin: 0 auto 12px;
      animation: spin 0.8s linear infinite;
    }
    .hint {
      font-size: 13px;
      color: #94a3b8;
    }
    @keyframes spin { to { transform: rotate(360deg); } }
  </style>
</head>
<body>
  <div class="shell">
    <div class="title">Wunder Desktop</div>
    <div class="spinner" aria-hidden="true"></div>
    <div class="hint">Starting local services...</div>
  </div>
</body>
</html>`

const createSafeModeHtml = () => `<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <meta http-equiv="x-ua-compatible" content="ie=edge" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Wunder Desktop Safe Mode</title>
  <style>
    :root { color-scheme: light dark; }
    body {
      margin: 0;
      font-family: "Segoe UI", system-ui, -apple-system, sans-serif;
      display: grid;
      place-items: center;
      height: 100vh;
      background: #f8fafc;
      color: #0f172a;
    }
    .shell {
      width: min(720px, calc(100vw - 32px));
      padding: 24px;
      border: 1px solid rgba(148, 163, 184, 0.4);
      border-radius: 12px;
      background: rgba(255, 255, 255, 0.94);
      box-shadow: 0 12px 40px rgba(15, 23, 42, 0.12);
    }
    .title {
      font-size: 18px;
      font-weight: 600;
      margin-bottom: 10px;
    }
    .desc {
      font-size: 14px;
      line-height: 1.7;
      color: #334155;
      word-break: break-word;
      white-space: pre-wrap;
    }
    .meta {
      margin-top: 14px;
      font-size: 12px;
      color: #64748b;
      word-break: break-word;
    }
  </style>
</head>
<body>
  <div class="shell">
    <div class="title">Wunder Desktop safe mode</div>
    <div class="desc">The renderer is loading a minimal diagnostic page.<br />Bridge and frontend bundle are disabled here.</div>
    <div class="meta">If this stays stable, the crash is in the normal desktop UI path.</div>
  </div>
</body>
</html>`

const createWindow = async () => {
  const createWindowNs = process.hrtime.bigint()
  logStartupPoint('electron', 'create_window_begin')
  const constructWindowNs = process.hrtime.bigint()
  mainWindow = new BrowserWindow({
    width: 1024,
    height: 700,
    minWidth: 900,
    minHeight: 620,
    title: 'Wunder Desktop',
    icon: resolveWindowIcon(),
    frame: desktopSafeModeEnabled ? true : false,
    show: false,
    autoHideMenuBar: true,
    ...(process.platform === 'darwin' && !desktopSafeModeEnabled ? { titleBarStyle: 'hidden' } : {}),
    ...(desktopSafeModeEnabled ? { backgroundColor: '#f8fafc' } : {}),
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      ...(desktopSafeModeEnabled ? {} : { preload: path.join(__dirname, 'preload.js') }),
      sandbox: true,
      spellcheck: false,
      // Keep throttling enabled by default. Disabling it can cause hidden frameless
      // windows on Windows to come back as blank until a resize/maximize repaint.
      backgroundThrottling: !disableBackgroundThrottling
    }
  })
  mainWindowSendReady = false
  pendingMainWindowMessages.length = 0
  logStartupSegment('electron', 'window_construct', constructWindowNs, {
    min_width: 900,
    min_height: 620
  })
  const scheduleWindowRepaint = () => {
    if (!mainWindow || mainWindow.isDestroyed() || mainWindow.webContents.isDestroyed()) {
      return
    }
    mainWindow.webContents.invalidate()
    setTimeout(() => {
      if (!mainWindow || mainWindow.isDestroyed() || mainWindow.webContents.isDestroyed()) {
        return
      }
      mainWindow.webContents.invalidate()
    }, 80)
  }
  mainWindow.setMenuBarVisibility(false)
  mainWindow.once('ready-to-show', () => {
    logStartupPoint('electron', 'window_ready_to_show')
    mainWindow.show()
    scheduleWindowRepaint()
    scheduleLinuxDesktopIntegration()
  })
  mainWindow.on('show', () => {
    mainWindowVisibilityGuard.clearManualMinimize()
    scheduleWindowRepaint()
  })
  mainWindow.on('restore', () => {
    mainWindowVisibilityGuard.clearManualMinimize()
    scheduleWindowRepaint()
  })
  mainWindow.on('minimize', () => {
    mainWindowVisibilityGuard.markManualMinimize()
  })
  mainWindow.on('close', (event) => {
    handleMainWindowClose(mainWindow, event)
  })
  mainWindow.on('closed', () => {
    mainWindowSendReady = false
    pendingMainWindowMessages.length = 0
    mainWindow = null
  })
  let mainUiLoadedLogged = false
  mainWindow.webContents.on('did-start-navigation', (_event, _url, isInPlace, isMainFrame) => {
    if (isMainFrame && !isInPlace) {
      mainWindowSendReady = false
    }
  })
  mainWindow.webContents.on('did-finish-load', () => {
    if (!mainWindow || mainWindow.isDestroyed() || mainWindow.webContents.isDestroyed()) {
      return
    }
    const currentUrl = mainWindow.webContents.getURL()
    if (!mainUiLoadedLogged && !currentUrl.startsWith('data:text/html')) {
      mainUiLoadedLogged = true
      logStartupPoint('electron', 'main_ui_loaded', {
        url: currentUrl
      })
    }
    if (!currentUrl.startsWith('data:text/html')) {
      mainWindowSendReady = true
      writeRendererCrashState({
        lastLoadedAt: Date.now(),
        lastReason: '',
        lastExitCode: 0
      })
      flushMainWindowMessages()
    }
  })
  mainWindow.webContents.on('render-process-gone', (_event, details) => {
    const reason = String(details?.reason || '').trim()
    const oomLike =
      reason === 'oom' ||
      Number(details?.exitCode) === -536870904 ||
      Number(details?.exitCode) === -1073741819
    writeRendererCrashState({
      compatibleGraphics: oomLike ? true : rendererCompatibilityModeEnabled,
      lastReason: reason,
      lastExitCode: Number(details?.exitCode) || 0,
      lastGoneAt: Date.now()
    })
    console.error('[desktop-debug][electron] render-process-gone', {
      reason,
      exitCode: details?.exitCode,
      killed: details?.killed === true,
      crashed: details?.reason === 'crashed',
      compatibilityModeNextStart: oomLike || rendererCompatibilityModeEnabled
    })
  })
  mainWindow.webContents.on('child-process-gone', (_event, details) => {
    console.error('[desktop-debug][electron] child-process-gone', {
      type: details?.type || '',
      reason: details?.reason || '',
      exitCode: details?.exitCode
    })
  })
  const reportMainWindowMemory = async () => {
    if (!mainWindow || mainWindow.isDestroyed() || mainWindow.webContents.isDestroyed()) {
      return
    }
    try {
      const rendererPid =
        typeof mainWindow.webContents.getOSProcessId === 'function'
          ? mainWindow.webContents.getOSProcessId()
          : 0
      if (rendererPid) {
        const metrics = typeof app.getAppMetrics === 'function' ? app.getAppMetrics() : []
        const rendererMetric = metrics.find((item) => Number(item?.pid || 0) === Number(rendererPid))
        const rendererMemory = rendererMetric?.memory
        if (rendererMemory) {
          console.info('[desktop-debug][electron] renderer-memory', {
            pid: rendererPid,
            type: rendererMetric.type || '',
            resident: rendererMemory.residentSet || 0,
            private: rendererMemory.private || 0,
            shared: rendererMemory.shared || 0
          })
        }
      }
      if (typeof process.getProcessMemoryInfo === 'function') {
        const mainMemory = await process.getProcessMemoryInfo()
        console.info('[desktop-debug][electron] main-process-memory', {
          private: mainMemory.private || 0,
          shared: mainMemory.shared || 0,
          resident: mainMemory.residentSet || 0
        })
      }
    } catch (error) {
      console.warn('[desktop-debug][electron] memory sampling failed', error?.message || error)
    }
  }
  const mainWindowMemoryTimer = setInterval(reportMainWindowMemory, 10000)
  mainWindow.once('closed', () => {
    clearInterval(mainWindowMemoryTimer)
  })
  if (desktopSafeModeEnabled) {
    const safeModeHtml = createSafeModeHtml()
    const loadSafeModeNs = process.hrtime.bigint()
    await mainWindow
      .loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(safeModeHtml)}`)
      .catch(() => {})
    logStartupPoint('electron', 'safe_mode_shell_loaded', {
      url: 'data:text/html;charset=utf-8'
    })
    logStartupSegment('electron', 'create_window_bootstrap_scheduled', loadSafeModeNs, {
      safe_mode: 1
    })
    return
  }

  const loadingHtml = createLoadingHtml()
  const bridgeReadyPromise = startBridge()
  let shellLoadStarted = false
  let targetLoadStarted = false
  let shellTimer = null
  const loadShellIfNeeded = async () => {
    if (shellLoadStarted || targetLoadStarted) {
      return
    }
    if (!mainWindow || mainWindow.isDestroyed()) {
      return
    }
    shellLoadStarted = true
    const loadShellNs = process.hrtime.bigint()
    await mainWindow
      .loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(loadingHtml)}`)
      .catch(() => {})
    logStartupSegment('electron', 'window_loading_shell_loaded', loadShellNs)
  }
  if (loadingShellDelayMs === 0) {
    void loadShellIfNeeded()
  } else {
    shellTimer = setTimeout(() => {
      void loadShellIfNeeded()
    }, loadingShellDelayMs)
  }

  const startBridgeAndLoad = async () => {
    const startBridgeAndLoadNs = process.hrtime.bigint()
    try {
      const bridgeReadyForWindowNs = process.hrtime.bigint()
      const port = await bridgeReadyPromise
      logStartupSegment('electron', 'bridge_ready_for_window', bridgeReadyForWindowNs, {
        port
      })
      if (shellTimer) {
        clearTimeout(shellTimer)
        shellTimer = null
      }
      const target = bridgeWebBase ? `${bridgeWebBase}/` : `http://127.0.0.1:${port}/`
      if (!mainWindow || mainWindow.isDestroyed()) {
        return
      }
      targetLoadStarted = true
      const loadTargetNs = process.hrtime.bigint()
      await mainWindow.loadURL(target)
      logStartupSegment('electron', 'window_target_loaded', loadTargetNs, {
        target
      })
      logStartupSegment('electron', 'start_bridge_and_load_total', startBridgeAndLoadNs)
    } catch (err) {
      if (shellTimer) {
        clearTimeout(shellTimer)
        shellTimer = null
      }
      logStartupPoint('electron', 'start_bridge_and_load_failed', {
        message: err?.message || String(err)
      })
      dialog.showErrorBox('Wunder Desktop', err?.message || String(err))
      app.quit()
    }
  }
  void startBridgeAndLoad()
  logStartupSegment('electron', 'create_window_bootstrap_scheduled', createWindowNs)
}

const gotLock = app.requestSingleInstanceLock()
if (!gotLock) {
  app.quit()
} else {
  const appWhenReadyWaitNs = process.hrtime.bigint()
  app.on('second-instance', () => {
    showMainWindow({ explicit: true })
  })

  app.whenReady().then(async () => {
    logStartupSegment('electron', 'app_when_ready', appWhenReadyWaitNs)
    try {
      const appCorePreinitNs = process.hrtime.bigint()
      configureUpdaterEvents()
      closeBehavior = loadCloseBehavior()
      configureMediaPermissions()
      logStartupSegment('electron', 'app_core_preinit', appCorePreinitNs)
      screen.on('display-added', updateOverlayBounds)
      screen.on('display-removed', updateOverlayBounds)
      screen.on('display-metrics-changed', updateOverlayBounds)
      screen.on('display-added', renderCompanionWindow)
      screen.on('display-removed', renderCompanionWindow)
      screen.on('display-metrics-changed', renderCompanionWindow)
      const registerIpcNs = process.hrtime.bigint()
      ipcMain.handle('wunder:toggle-devtools', () => toggleMainDevTools())
      ipcMain.handle('wunder:window-minimize', () =>
        withMainWindow((window) => {
          mainWindowVisibilityGuard.markManualMinimize()
          window.minimize()
          return true
        }, false)
      )
      ipcMain.handle('wunder:window-toggle-maximize', () =>
        withMainWindow((window) => {
          if (window.isMaximized()) {
            window.unmaximize()
          } else {
            window.maximize()
          }
          return window.isMaximized()
        }, false)
      )
      ipcMain.handle('wunder:window-close', () =>
        withMainWindow((window) => {
          window.close()
          return true
        }, false)
      )
      ipcMain.handle('wunder:window-is-maximized', () =>
        withMainWindow((window) => window.isMaximized(), false)
      )
      ipcMain.handle('wunder:window-close-behavior-get', () => sanitizeCloseBehavior(closeBehavior))
      ipcMain.handle('wunder:window-close-behavior-set', (_event, payload) => {
        const source =
          payload && typeof payload === 'object' ? payload.behavior ?? payload.closeBehavior : payload
        const text = String(source || '')
          .trim()
          .toLowerCase()
        const requested = text === 'hide' ? 'tray' : text
        const normalized = sanitizeCloseBehavior(requested)
        saveCloseBehavior(normalized)
        return sanitizeCloseBehavior(closeBehavior)
      })
      ipcMain.handle('wunder:launch-at-login-get', () => getLaunchAtLoginState())
      ipcMain.handle('wunder:launch-at-login-set', (_event, payload) => {
        const enabled =
          payload && typeof payload === 'object' ? payload.enabled === true : payload === true
        return setLaunchAtLoginState(enabled)
      })
      ipcMain.handle('wunder:python-runtime-info', () => resolveDesktopPythonRuntimeInfo())
      ipcMain.handle('wunder:renderer-stage', (_event, payload) => {
        const source = payload && typeof payload === 'object' ? payload : {}
        const stage = String(source.stage || '').trim().slice(0, 96)
        if (!stage) {
          return false
        }
        console.info('[desktop-debug][renderer-stage]', {
          stage,
          ...sanitizeRendererStagePayload(source.payload)
        })
        return true
      })
      ipcMain.handle('wunder:bridge-restart', () => restartBridge())
      ipcMain.handle('wunder:supplement-import', () => importSupplementPackage())
      ipcMain.handle('wunder:open-path-default-app', (_event, payload) => {
        const source = payload && typeof payload === 'object' ? payload.path : payload
        return openPathWithDefaultApp(source)
      })
      ipcMain.handle('wunder:window-start-drag', () => false)
      ipcMain.handle('wunder:clipboard-write-text', (_event, payload) => {
        const text =
          payload && typeof payload === 'object' ? String(payload.text || '') : String(payload || '')
        if (!text.trim()) {
          return false
        }
        clipboard.writeText(text)
        return true
      })
      ipcMain.handle('wunder:media-access-status', (_event, payload) => {
        const kind = payload && typeof payload === 'object' ? payload.kind : payload
        return getMediaAccessStatus(kind)
      })
      ipcMain.handle('wunder:media-request-access', async (_event, payload) => {
        const kind = payload && typeof payload === 'object' ? payload.kind : payload
        return requestMediaAccess(kind)
      })
      ipcMain.handle('wunder:notify', (_event, payload) => showDesktopNotification(payload))
      ipcMain.handle('wunder:update-check', () => checkAndDownloadUpdate())
      ipcMain.handle('wunder:update-status', () => getUpdateState())
      ipcMain.handle('wunder:update-install', () => installDownloadedUpdate())
      ipcMain.handle('wunder:capture-screenshot', async (_event, payload) => {
        const hideWindow =
          payload && typeof payload === 'object'
            ? payload.hideWindow === true
            : false
        const region =
          payload && typeof payload === 'object'
            ? payload.region === true
            : false
        return captureDesktopScreenshot({ hideWindow, region })
      })
      ipcMain.handle('wunder:choose-python-interpreter', async (_event, payload) => {
        const rawDefaultPath =
          payload && typeof payload === 'object' ? String(payload.defaultPath || '').trim() : ''
        return choosePythonInterpreter(rawDefaultPath)
      })
      ipcMain.handle('wunder:choose-runtime-executable', async (_event, payload) => {
        return chooseRuntimeExecutable(payload && typeof payload === 'object' ? payload : {})
      })
      ipcMain.handle('wunder:choose-directory', async (_event, payload) => {
        const rawDefaultPath =
          payload && typeof payload === 'object' ? String(payload.defaultPath || '').trim() : ''
        const result = await withMainWindow(
          (window) =>
            dialog.showOpenDialog(window, {
              title: 'Select Local Directory',
              defaultPath: rawDefaultPath || undefined,
              properties: ['openDirectory', 'createDirectory']
            }),
          () =>
            dialog.showOpenDialog({
              title: 'Select Local Directory',
              defaultPath: rawDefaultPath || undefined,
              properties: ['openDirectory', 'createDirectory']
            })
        )
        if (result?.canceled || !Array.isArray(result?.filePaths) || !result.filePaths.length) {
          return ''
        }
        return String(result.filePaths[0] || '').trim()
      })
      ipcMain.handle('wunder:overlay-controller-hint', (_event, payload) =>
        showControllerOverlay(payload, 'pending')
      )
      ipcMain.handle('wunder:overlay-controller-done', (_event, payload) =>
        showControllerOverlay(payload, 'done')
      )
      ipcMain.handle('wunder:overlay-monitor-countdown', (_event, payload) =>
        showMonitorOverlay(payload)
      )
      ipcMain.handle('wunder:overlay-hide', () => {
        hideOverlayNow()
        return true
      })
      ipcMain.handle('wunder:companion-show', (_event, payload) => showCompanion(payload))
      ipcMain.handle('wunder:companion-update', (_event, payload) => updateCompanion(payload))
      ipcMain.handle('wunder:companion-hide', (_event, payload) => hideCompanion(payload))
      ipcMain.handle('wunder:companion-state', () => loadCompanionState())
      ipcMain.handle('wunder:companion-library-state-get', () => loadCompanionLibraryState())
      ipcMain.handle('wunder:companion-library-state-set', (_event, payload) => {
        saveCompanionLibraryState(payload)
        return true
      })
      ipcMain.handle('wunder:companion-drag', (event, payload) => moveCompanionBy(payload, event))
      ipcMain.handle('wunder:companion-drag-end', (event, payload) => endCompanionDrag(payload, event))
      ipcMain.handle('wunder:companion-pointer-events', (event, payload) => setCompanionPointerEvents(payload, event))
      ipcMain.handle('wunder:companion-hit-shape', (event, payload) => updateCompanionHitShape(payload, event))
      ipcMain.handle(COMPANION_COMMAND_CHANNEL, (_event, payload) => emitCompanionCommand(payload))
      logStartupSegment('electron', 'app_ipc_handlers_registered', registerIpcNs)
      Menu.setApplicationMenu(null)
      createTray()
      const createWindowCallNs = process.hrtime.bigint()
      await createWindow()
      logStartupSegment('electron', 'app_create_window_returned', createWindowCallNs)
      logStartupPoint('electron', 'bootstrap_pipeline_done')
    } catch (err) {
      logStartupPoint('electron', 'bootstrap_pipeline_failed', {
        message: err?.message || String(err)
      })
      dialog.showErrorBox('Wunder Desktop', err?.message || String(err))
      app.quit()
    }
  })
}

app.on('before-quit', () => {
  app.isQuitting = true
  hideCompanion()
  destroyTray()
  stopBridge()
})

app.on('window-all-closed', () => {
  app.quit()
})
