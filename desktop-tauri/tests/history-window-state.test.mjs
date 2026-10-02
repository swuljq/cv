import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import {
  readWindowPinned,
  shouldAutoHideHistory,
} from '../src/history-window-state.ts';

test('history window auto-hides only when it is not pinned', () => {
  assert.equal(shouldAutoHideHistory(false), true);
  assert.equal(shouldAutoHideHistory(true), false);
  assert.equal(shouldAutoHideHistory(false, true), false);
});

test('only an explicit persisted true value restores the pinned state', () => {
  assert.equal(readWindowPinned('true'), true);
  assert.equal(readWindowPinned('false'), false);
  assert.equal(readWindowPinned(null), false);
  assert.equal(readWindowPinned('invalid'), false);
});

test('all history window hide paths use the native command', async () => {
  const source = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');

  assert.doesNotMatch(source, /currentWindow\.hide\(/);
  assert.equal(source.match(/invoke\('hide_history'\)/g)?.length, 3);
});

test('history window has permission to start native dragging', async () => {
  const raw = await readFile(new URL('../src-tauri/capabilities/default.json', import.meta.url), 'utf8');
  const capability = JSON.parse(raw);

  assert.ok(capability.windows.includes('history'));
  assert.ok(capability.permissions.includes('core:window:allow-start-dragging'));
  assert.ok(capability.permissions.includes('dialog:allow-open'));
});

test('history opacity controls record text and card backgrounds', async () => {
  const styles = await readFile(new URL('../src/styles.css', import.meta.url), 'utf8');

  assert.match(styles, /\.history-item\{[^}]*color:rgb\(23 32 51 \/ var\(--history-opacity,/);
  assert.match(styles, /\.history-item\{[^}]*background:rgb\(248 250 252 \/ var\(--history-opacity,/);
  assert.match(styles, /\.history-item:hover\{[^}]*background:rgb\(224 234 255 \/ var\(--history-opacity,/);
});

test('history items expose their full text on hover', async () => {
  const source = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');

  assert.match(source, /const title = item\.kind === 'text' \? escapeHtml\(item\.text \|\| ''\)/);
  assert.match(source, /title="\$\{title\}"/);
});

test('image history uses lazy-loaded thumbnails and ID selection', async () => {
  const source = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');
  const styles = await readFile(new URL('../src/styles.css', import.meta.url), 'utf8');

  assert.match(source, /invoke<HistoryItem\[]>\('get_clipboard_history'\)/);
  assert.match(source, /invoke<string>\('get_history_thumbnail', \{ id \}\)/);
  assert.match(source, /new IntersectionObserver/);
  assert.match(source, /invoke\('select_clipboard_history', \{ id: button\.dataset\.id \}\)/);
  assert.match(styles, /\.history-thumbnail\{/);
});

test('history list fills the window and scrolls independently', async () => {
  const styles = await readFile(new URL('../src/styles.css', import.meta.url), 'utf8');

  assert.match(styles, /body\.history-body #app\{height:100vh\}/);
  assert.match(styles, /\.history-window\{display:flex;flex-direction:column;/);
  assert.match(styles, /#history-list\{min-height:0;flex:1;overflow-y:auto;/);
});

test('visible history window refreshes when clipboard history changes', async () => {
  const source = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');

  assert.match(source, /listen\('clipboard-history-changed', renderHistory\)/);
});

test('main window exposes ClipSync local history settings', async () => {
  const source = await readFile(new URL('../src/main.ts', import.meta.url), 'utf8');
  const config = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
  const html = await readFile(new URL('../index.html', import.meta.url), 'utf8');

  assert.match(source, /<h2>ClipSync<\/h2>/);
  assert.match(source, /id="history-limit" type="number" min="1" max="1000" value="10"/);
  assert.match(source, /id="history-directory" readonly/);
  assert.match(source, /open\(\{ directory: true, multiple: false/);
  assert.match(source, /invoke<HistorySettings>\('save_history_settings'/);
  assert.doesNotMatch(source, /连接后可最小化到后台/);
  assert.equal(config.productName, 'ClipSync');
  assert.equal(config.app.windows[0].title, 'ClipSync');
  assert.match(html, /<title>ClipSync<\/title>/);
});
