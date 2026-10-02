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
});

test('history opacity controls record text color', async () => {
  const styles = await readFile(new URL('../src/styles.css', import.meta.url), 'utf8');

  assert.match(styles, /\.history-item\{[^}]*color:rgb\(23 32 51 \/ var\(--history-opacity,/);
});
