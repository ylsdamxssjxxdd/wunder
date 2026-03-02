const { app, BrowserWindow, dialog, Menu, Tray, nativeImage, ipcMain } = require('electron')
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

const parseBridgePort = (line) => {
  const trimmed = line.trim()
  const match = trimmed.match(/- (web_base|api_base):\s*(https?:\/\/\S+)/)
  if (!match) {
    return null
  }
  try {
    const url = new URL(match[2])
    if (!url.port) {
      return null
    }
    bridgeWebBase = url.origin
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
    width: 1360,
    height: 860,
    minWidth: 1024,
    minHeight: 700,
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
      backgroundThrottling: false
    }
  })
  mainWindow.setMenuBarVisibility(false)
  mainWindow.once('ready-to-show', () => {
    mainWindow.show()
  })
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
      ipcMain.handle('wunder:window-start-drag', () => false)
      ipcMain.handle('wunder:update-check', () => checkAndDownloadUpdate())
      ipcMain.handle('wunder:update-status', () => getUpdateState())
      ipcMain.handle('wunder:update-install', () => installDownloadedUpdate())
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
