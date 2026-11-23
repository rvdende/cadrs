const { contextBridge, ipcRenderer } = require('electron');

// scrape.js hands its responses to the app through this instead of fetching http://127.0.0.1.
contextBridge.exposeInMainWorld('cadrsBridge', {
  sink: (route, path, method, body) => ipcRenderer.invoke('sink', { route, path, method, body }),
});
