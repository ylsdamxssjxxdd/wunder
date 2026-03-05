const {
  app,
  BrowserWindow,
  dialog,
  Menu,
  Tray,
  nativeImage,
  ipcMain,
  desktopCapturer,
  screen,
  Notification
} = require('electron')
const { autoUpdater } = require('electron-updater')
const { spawn } = require('child_process')
const fs = require('fs')
const http = require('http')
const net = require('net')
const path = require('path')

let mainWindow = null
let bridgeProcess = null
let bridgePort = null
let bridgeWebBase = null
let updateTask = null
let updaterReady = false
let tray = null
let closePromptInFlight = false
let closeBehavior = 'ask'
const disableBackgroundThrottling = process.env.WUNDER_DISABLE_BACKGROUND_THROTTLING === '1'

const SCREENSHOT_HIDE_DELAY_MS = 220
const SCREENSHOT_SELECTOR_RESULT_CHANNEL = 'wunder:screenshot-region-selected'
const SCREENSHOT_SELECTOR_CANCEL_CHANNEL = 'wunder:screenshot-region-canceled'

const createUpdateSnapshot = () => ({
  phase: 'idle',
  currentVersion: app.getVersion(),
  latestVersion: '',
  downloaded: false,
  progress: 0,
  message: ''
})

let updateState = createUpdateSnapshot()

app.commandLine.appendSwitch('log-level', '2')
if (process.platform === 'linux') {
  app.commandLine.appendSwitch('class', 'wunder-desktop')
}
if (process.env.WUNDER_DISABLE_GPU === '1') {
  app.disableHardwareAcceleration()
}

const repoRoot = path.resolve(__dirname, '..', '..')
const localResourcesRoot = path.resolve(__dirname, '..', 'resources')
const desktopAppId = 'com.wunder.desktop'
const closePreferenceFileName = 'window-close-preference.json'
const closeBehaviorValues = new Set(['ask', 'tray', 'quit'])

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
    if (!app.isPackaged) {
      setUpdateState({
        phase: 'unsupported',
        message: 'auto update is only available in packaged app'
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
  if (!app.isPackaged || !updateState.downloaded || updateState.phase !== 'downloaded') {
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
    if (shouldRestore && window && !window.isDestroyed()) {
      window.show()
      window.focus()
    }
  }
}

const waitForBridge = (resolvePort, timeoutMs = 15000) =>
  new Promise((resolve, reject) => {
    const startedAt = Date.now()
    const attempt = () => {
      const port = resolvePort()
      if (!port) {
        retry()
        return
      }
      const req = http.get(
        {
          hostname: '127.0.0.1',
          port,
          path: '/config.json',
          timeout: 2000
        },
        (res) => {
          res.resume()
          if (res.statusCode === 200) {
            resolve()
            return
          }
          retry()
        }
      )
      req.on('error', retry)
      req.on('timeout', () => {
        req.destroy()
        retry()
      })
    }
    const retry = () => {
      if (Date.now() - startedAt > timeoutMs) {
        reject(new Error('Bridge did not respond in time'))
        return
      }
      setTimeout(attempt, 300)
    }
    attempt()
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
  const bridgePath = resolveBridgePath()
  if (!fs.existsSync(bridgePath)) {
    throw new Error(`Bridge binary not found: ${bridgePath}`)
  }

  const frontendRoot = resolveFrontendRoot()
  const tempRoot = path.join(app.getPath('userData'), 'WUNDER_TEMPD')
  const workspaceRoot = path.join(app.getPath('userData'), 'WUNDER_WORK')
  bridgePort = await getFreePort()

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

  if (frontendRoot && fs.existsSync(frontendRoot)) {
    args.push('--frontend-root', frontendRoot)
  }

  bridgeProcess = spawn(bridgePath, args, {
    env: { ...process.env },
    stdio: ['ignore', 'pipe', 'pipe']
  })

  bridgeProcess.stdout.on('data', (data) => {
    const text = data.toString()
    const lines = text.split(/\r?\n/)
    for (const line of lines) {
      const trimmed = line.trim()
      if (!trimmed) {
        continue
      }
      console.log(`[bridge] ${trimmed}`)
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

  await waitForBridge(() => bridgePort)
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

const showMainWindow = () =>
  withMainWindow((window) => {
    if (window.isMinimized()) {
      window.restore()
    }
    if (!window.isVisible()) {
      window.show()
    }
    window.focus()
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
        showMainWindow()
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
    showMainWindow()
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

const createWindow = async () => {
  const port = await startBridge()
  mainWindow = new BrowserWindow({
    width: 1024,
    height: 700,
    minWidth: 900,
    minHeight: 620,
    title: 'Wunder Desktop',
    icon: resolveWindowIcon(),
    frame: false,
    show: false,
    autoHideMenuBar: true,
    ...(process.platform === 'darwin' ? { titleBarStyle: 'hidden' } : {}),
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      preload: path.join(__dirname, 'preload.js'),
      sandbox: true,
      spellcheck: false,
      // Keep throttling enabled by default. Disabling it can cause hidden frameless
      // windows on Windows to come back as blank until a resize/maximize repaint.
      backgroundThrottling: !disableBackgroundThrottling
    }
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
    mainWindow.show()
    scheduleWindowRepaint()
  })
  mainWindow.on('show', scheduleWindowRepaint)
  mainWindow.on('restore', scheduleWindowRepaint)
  mainWindow.on('close', (event) => {
    handleMainWindowClose(mainWindow, event)
  })
  mainWindow.on('closed', () => {
    mainWindow = null
  })
  const target = bridgeWebBase ? `${bridgeWebBase}/` : `http://127.0.0.1:${port}/`
  await mainWindow.loadURL(target)
}

const gotLock = app.requestSingleInstanceLock()
if (!gotLock) {
  app.quit()
} else {
  app.on('second-instance', () => {
    showMainWindow()
  })

  app.whenReady().then(async () => {
    try {
      configureUpdaterEvents()
      ensureLinuxDesktopIntegration()
      closeBehavior = loadCloseBehavior()
      ipcMain.handle('wunder:toggle-devtools', () => toggleMainDevTools())
      ipcMain.handle('wunder:window-minimize', () =>
        withMainWindow((window) => {
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
      ipcMain.handle('wunder:window-start-drag', () => false)
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
      Menu.setApplicationMenu(null)
      await createWindow()
    } catch (err) {
      dialog.showErrorBox('Wunder Desktop', err?.message || String(err))
      app.quit()
    }
  })
}

app.on('before-quit', () => {
  app.isQuitting = true
  destroyTray()
  stopBridge()
})

app.on('window-all-closed', () => {
  app.quit()
})
