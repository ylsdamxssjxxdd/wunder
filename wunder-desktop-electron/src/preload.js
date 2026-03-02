const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('wunderDesktop', {
  toggleDevTools: () => ipcRenderer.invoke('wunder:toggle-devtools'),
  minimizeWindow: () => ipcRenderer.invoke('wunder:window-minimize'),
  toggleMaximizeWindow: () => ipcRenderer.invoke('wunder:window-toggle-maximize'),
  closeWindow: () => ipcRenderer.invoke('wunder:window-close'),
  isWindowMaximized: () => ipcRenderer.invoke('wunder:window-is-maximized'),
  startWindowDrag: () => ipcRenderer.invoke('wunder:window-start-drag'),
  checkForUpdates: () => ipcRenderer.invoke('wunder:update-check'),
  getUpdateState: () => ipcRenderer.invoke('wunder:update-status'),
  installUpdate: () => ipcRenderer.invoke('wunder:update-install'),
  captureScreenshot: (options) => ipcRenderer.invoke('wunder:capture-screenshot', options || {}),
  chooseDirectory: (defaultPath) => ipcRenderer.invoke('wunder:choose-directory', { defaultPath })
})
