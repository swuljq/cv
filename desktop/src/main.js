const { app, BrowserWindow, clipboard, nativeImage, ipcMain } = require('electron');
const path = require('node:path');
const crypto = require('node:crypto');
const WebSocket = require('ws');

let win;
let socket;
let suppressUntil = 0;
let lastHash = '';

function hash(value) { return crypto.createHash('sha256').update(value).digest('hex'); }
function sendClipboard(contentType, data) {
  if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: 'clipboard', eventId: crypto.randomUUID(), contentType, data }));
}
function pollClipboard() {
  if (Date.now() < suppressUntil) return;
  const text = clipboard.readText();
  if (text) {
    const current = `text:${hash(text)}`;
    if (current !== lastHash) { lastHash = current; sendClipboard('text', text); }
    return;
  }
  const image = clipboard.readImage();
  if (!image.isEmpty()) {
    const data = image.toPNG().toString('base64');
    const current = `image:${hash(data)}`;
    if (current !== lastHash) { lastHash = current; sendClipboard('image', data); }
  }
}
function connect(settings) {
  socket?.close();
  socket = new WebSocket(settings.serverUrl);
  socket.on('open', () => socket.send(JSON.stringify({ type: 'auth', username: settings.username, password: settings.password, deviceId: settings.deviceId, deviceName: 'Windows desktop' })));
  socket.on('message', raw => {
    const message = JSON.parse(raw.toString());
    if (message.type === 'auth_ok') win.webContents.send('status', '已连接');
    if (message.type === 'error') win.webContents.send('status', '认证失败');
    if (message.type === 'clipboard') {
      suppressUntil = Date.now() + 1000;
      if (message.contentType === 'text') clipboard.writeText(message.data);
      if (message.contentType === 'image') clipboard.writeImage(nativeImage.createFromBuffer(Buffer.from(message.data, 'base64')));
      lastHash = `${message.contentType}:${hash(message.data)}`;
      win.webContents.send('status', `已同步 ${message.contentType === 'text' ? '文本' : '图片'}`);
    }
  });
  socket.on('close', () => win.webContents.send('status', '连接断开'));
  socket.on('error', () => win.webContents.send('status', '连接错误'));
}
function createWindow() {
  win = new BrowserWindow({ width: 420, height: 360, resizable: false, webPreferences: { preload: path.join(__dirname, 'preload.js') } });
  win.loadFile(path.join(__dirname, 'index.html'));
}
app.whenReady().then(() => { createWindow(); setInterval(pollClipboard, 500); });
ipcMain.on('connect', (_, settings) => connect(settings));
