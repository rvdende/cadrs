const { contextBridge, ipcRenderer } = require('electron');

// scrape.js hands its responses to the app through this instead of fetching http://127.0.0.1.
contextBridge.exposeInMainWorld('cadrsBridge', {
  sink: (route, path, method, body) => ipcRenderer.invoke('sink', { route, path, method, body }),
});

// Onshape says "the WebGL rendering context has been lost" after a GPU driver reset; that need
// not kill the GPU process, so report the page's own signal (the event does not bubble, hence the
// capture listener) and let the app reload the pane or suggest software rendering.
window.addEventListener('webglcontextlost', () => ipcRenderer.send('onshape-context-lost'), true);
window.addEventListener('webglcontextrestored', () => ipcRenderer.send('onshape-context-restored'), true);
