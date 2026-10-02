import assert from 'node:assert/strict';
import test from 'node:test';
import {
  readWindowPinned,
  shouldAutoHideHistory,
} from '../src/history-window-state.ts';

test('history window auto-hides only when it is not pinned', () => {
  assert.equal(shouldAutoHideHistory(false), true);
  assert.equal(shouldAutoHideHistory(true), false);
});

test('only an explicit persisted true value restores the pinned state', () => {
  assert.equal(readWindowPinned('true'), true);
  assert.equal(readWindowPinned('false'), false);
  assert.equal(readWindowPinned(null), false);
  assert.equal(readWindowPinned('invalid'), false);
});
