const { contextBridge, ipcRenderer } = require('electron');
contextBridge.exposeInMainWorld('clipbridge', { connect: settings => ipcRenderer.send('connect', settings), onStatus: callback => ipcRenderer.on('status', (_, value) => callback(value)) });
