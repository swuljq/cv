import http from 'node:http';
import { WebSocketServer } from 'ws';

export const DEFAULT_USER = process.env.CLIPBRIDGE_USER || 'clipbridge';
export const DEFAULT_PASSWORD = process.env.CLIPBRIDGE_PASSWORD || 'clipbridge-dev';

export function createServer({ port = Number(process.env.PORT || 8787), host = process.env.HOST || '127.0.0.1', user = DEFAULT_USER, password = DEFAULT_PASSWORD } = {}) {
  const clients = new Map();
  const httpServer = http.createServer((req, res) => {
    if (req.url === '/health') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ ok: true, clients: clients.size }));
      return;
    }
    res.writeHead(404);
    res.end();
  });
  const wss = new WebSocketServer({ server: httpServer });
  const heartbeat = setInterval(() => {
    for (const socket of wss.clients) {
      if (socket.isAlive === false) {
        socket.terminate();
        continue;
      }
      socket.isAlive = false;
      socket.ping();
    }
  }, 30000);
  heartbeat.unref?.();
  wss.on('connection', socket => {
    socket.isAlive = true;
    socket.on('pong', () => { socket.isAlive = true; });
    let client;
    socket.on('message', raw => {
      let message;
      try { message = JSON.parse(raw.toString()); } catch { return; }
      if (!client) {
        if (message.type !== 'auth' || message.username !== user || message.password !== password || !message.deviceId) {
          socket.send(JSON.stringify({ type: 'error', code: 'AUTH_FAILED' }));
          socket.close();
          return;
        }
        client = { socket, deviceId: message.deviceId, deviceName: message.deviceName || message.deviceId };
        clients.set(client.deviceId, client);
        socket.send(JSON.stringify({ type: 'auth_ok', deviceId: client.deviceId }));
        return;
      }
      if (message.type === 'clipboard' && (message.contentType === 'text' || message.contentType === 'image') && typeof message.data === 'string') {
        const event = { type: 'clipboard', eventId: message.eventId, contentType: message.contentType, data: message.data, sourceDeviceId: client.deviceId, createdAt: new Date().toISOString() };
        for (const target of clients.values()) {
          if (target.deviceId !== client.deviceId && target.socket.readyState === target.socket.OPEN) target.socket.send(JSON.stringify(event));
        }
      }
    });
    socket.on('close', () => { if (client && clients.get(client.deviceId)?.socket === socket) clients.delete(client.deviceId); });
  });
  return { httpServer, wss, clients, listen: () => new Promise(resolve => httpServer.listen(port, host, resolve)), close: () => new Promise(resolve => { clearInterval(heartbeat); wss.close(() => httpServer.close(resolve)); }) };
}

if (process.argv[1] && new URL(import.meta.url).pathname === new URL(`file://${process.argv[1].replaceAll('\\', '/')}`).pathname) {
  const server = createServer({ port: Number(process.env.PORT || 8787) });
  server.listen().then(() => console.log(`ClipBridge server listening on ws://${process.env.HOST || '127.0.0.1'}:${process.env.PORT || 8787}`));
}
