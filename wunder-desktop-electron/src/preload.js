const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('wunderDesktop', {
  toggleDevTools: () => ipcRenderer.invoke('wunder:toggle-devtools')
})
