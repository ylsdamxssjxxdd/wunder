const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('wunderDesktop', {
  toggleDevTools: () => ipcRenderer.invoke('wunder:toggle-devtools'),
  minimizeWindow: () => ipcRenderer.invoke('wunder:window-minimize'),
  toggleMaximizeWindow: () => ipcRenderer.invoke('wunder:window-toggle-maximize'),
  closeWindow: () => ipcRenderer.invoke('wunder:window-close'),
  isWindowMaximized: () => ipcRenderer.invoke('wunder:window-is-maximized'),
  getWindowCloseBehavior: () => ipcRenderer.invoke('wunder:window-close-behavior-get'),
  setWindowCloseBehavior: (behavior) =>
    ipcRenderer.invoke('wunder:window-close-behavior-set', { behavior }),
  getLaunchAtLogin: () => ipcRenderer.invoke('wunder:launch-at-login-get'),
  setLaunchAtLogin: (enabled) => ipcRenderer.invoke('wunder:launch-at-login-set', { enabled }),
  startWindowDrag: () => ipcRenderer.invoke('wunder:window-start-drag'),
  checkForUpdates: () => ipcRenderer.invoke('wunder:update-check'),
  getUpdateState: () => ipcRenderer.invoke('wunder:update-status'),
  installUpdate: () => ipcRenderer.invoke('wunder:update-install'),
  copyText: (text) => ipcRenderer.invoke('wunder:clipboard-write-text', { text }),
  getMediaAccessStatus: (kind) => ipcRenderer.invoke('wunder:media-access-status', { kind }),
  requestMediaAccess: (kind) => ipcRenderer.invoke('wunder:media-request-access', { kind }),
  notify: (payload) => ipcRenderer.invoke('wunder:notify', payload || {}),
  captureScreenshot: (options) => ipcRenderer.invoke('wunder:capture-screenshot', options || {}),
  chooseDirectory: (defaultPath) => ipcRenderer.invoke('wunder:choose-directory', { defaultPath }),
  showControllerHint: (payload) =>
    ipcRenderer.invoke('wunder:overlay-controller-hint', payload || {}),
  showControllerDone: (payload) =>
    ipcRenderer.invoke('wunder:overlay-controller-done', payload || {}),
  showMonitorCountdown: (payload) =>
    ipcRenderer.invoke('wunder:overlay-monitor-countdown', payload || {}),
  hideOverlay: () => ipcRenderer.invoke('wunder:overlay-hide')
})
