import test from 'node:test';
import assert from 'node:assert/strict';
import WebSocket from 'ws';
import { createServer } from '../src/server.js';

test('authenticated clients receive clipboard events from another device', async t => {
  const server = createServer({ port: 0, user: 'u', password: 'p' });
  await server.listen();
  const port = server.httpServer.address().port;
  t.after(() => server.close());
  const connect = deviceId => new Promise((resolve, reject) => {
    const socket = new WebSocket(`ws://127.0.0.1:${port}`);
    socket.once('open', () => socket.send(JSON.stringify({ type: 'auth', username: 'u', password: 'p', deviceId })));
    socket.once('message', () => resolve(socket));
    socket.once('error', reject);
  });
  const first = await connect('a');
  const second = await connect('b');
  const received = new Promise(resolve => second.once('message', data => resolve(JSON.parse(data.toString()))));
  first.send(JSON.stringify({ type: 'clipboard', eventId: 'e1', contentType: 'text', data: 'hello' }));
  assert.equal((await received).data, 'hello');
  first.close(); second.close();
});
