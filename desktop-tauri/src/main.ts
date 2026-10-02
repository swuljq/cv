import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { readWindowPinned, shouldAutoHideHistory, WINDOW_PINNED_STORAGE_KEY } from './history-window-state';
import './styles.css';

const app = document.querySelector<HTMLDivElement>('#app')!;
const currentWindow = getCurrentWindow();
const isHistoryWindow = currentWindow.label === 'history';
const escapeHtml = (value: string) => value.replace(/[&<>'"]/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' }[character] || character));

if (isHistoryWindow) {
  document.body.classList.add('history-body');
  app.innerHTML = `<section class="history-window"><div class="history-head"><button id="toggle-window-pin" type="button"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 4v5l3 3v2h-4v6l-1 1-1-1v-6H7v-2l3-3V4z"/></svg></button><button id="close-history" type="button" aria-label="关闭">×</button></div><div id="history-list"></div></section>`;
  document.querySelector<HTMLElement>('.history-head')!.onpointerdown = event => {
    if ((event.target as HTMLElement).closest('button')) return;
    void currentWindow.startDragging();
  };
  const windowPinButton = document.querySelector<HTMLButtonElement>('#toggle-window-pin')!;
  let windowPinned = readWindowPinned(localStorage.getItem(WINDOW_PINNED_STORAGE_KEY));
  const renderWindowPin = () => {
    const label = windowPinned ? '取消固定窗口' : '固定窗口';
    windowPinButton.classList.toggle('active', windowPinned);
    windowPinButton.setAttribute('aria-label', label);
    windowPinButton.setAttribute('aria-pressed', String(windowPinned));
    windowPinButton.title = label;
  };
  windowPinButton.onclick = () => {
    windowPinned = !windowPinned;
    localStorage.setItem(WINDOW_PINNED_STORAGE_KEY, String(windowPinned));
    renderWindowPin();
  };
  renderWindowPin();
  const historyList = document.querySelector<HTMLElement>('#history-list')!;
  const renderHistory = async () => {
    const items = await invoke<string[]>('get_clipboard_history');
    const pinned = new Set(JSON.parse(localStorage.getItem('clipbridge-pinned') || '[]') as string[]);
    const ordered = [...items].sort((a, b) => Number(pinned.has(b)) - Number(pinned.has(a)));
    historyList.innerHTML = ordered.length ? ordered.map((item, index) => `<div class="history-row"><button class="history-item" data-index="${index}">${escapeHtml(item)}</button><button class="pin-item" data-value="${encodeURIComponent(item)}" title="置顶">${pinned.has(item) ? '★' : '☆'}</button></div>`).join('') : '<p class="empty">还没有复制记录</p>';
    historyList.querySelectorAll<HTMLButtonElement>('.history-item').forEach(button => {
      button.onclick = async () => {
        await invoke('select_clipboard_history', { value: ordered[Number(button.dataset.index)] });
        if (shouldAutoHideHistory(windowPinned)) await currentWindow.hide();
      };
    });
    historyList.querySelectorAll<HTMLButtonElement>('.pin-item').forEach(button => {
      button.onclick = () => {
        const item = decodeURIComponent(button.dataset.value || '');
        if (pinned.has(item)) pinned.delete(item); else pinned.add(item);
        localStorage.setItem('clipbridge-pinned', JSON.stringify([...pinned]));
        void renderHistory();
      };
    });
    const opacity = Number(localStorage.getItem('clipbridge-opacity') || '92') / 100;
    document.querySelector<HTMLElement>('.history-window')!.style.setProperty('--history-opacity', String(opacity));
  };
  document.querySelector<HTMLButtonElement>('#close-history')!.onclick = async event => {
    event.stopPropagation();
    await invoke('hide_history');
  };
  void currentWindow.onFocusChanged(({ payload: focused }) => {
    if (!focused && shouldAutoHideHistory(windowPinned)) void currentWindow.hide();
  });
  void listen('clipboard-history-open', renderHistory);
  void listen<number>('history-opacity-changed', event => {
    document.querySelector<HTMLElement>('.history-window')!.style.setProperty('--history-opacity', String(event.payload));
  });
} else {
  app.innerHTML = `<main><h2>ClipBridge</h2><input id="server" value="ws://104.233.216.159:8787" placeholder="服务器地址"><input id="user" value="clipbridge" placeholder="账号"><input id="password" value="JocK36vOBqd4" type="password" placeholder="密码"><button id="connect">连接并开始同步</button><p id="status">未连接</p><label class="opacity-control">历史窗口透明度 <input id="opacity" type="range" min="5" max="100" value="92"><span id="opacity-value">92%</span></label><small>连接后可最小化到后台，按 Ctrl+Alt+Z 查看鼠标附近的历史窗口。</small></main>`;
  const $ = (id: string) => document.querySelector<HTMLInputElement | HTMLButtonElement | HTMLParagraphElement>(`#${id}`)!;
  ($('connect') as HTMLButtonElement).onclick = async () => {
    $('status').textContent = '连接中…';
    try { await invoke('start_sync', { url: ($('server') as HTMLInputElement).value, username: ($('user') as HTMLInputElement).value, password: ($('password') as HTMLInputElement).value }); $('status').textContent = '已连接，后台同步中'; }
    catch (error) { $('status').textContent = `连接失败：${error}`; }
  };
  const opacityInput = document.querySelector<HTMLInputElement>('#opacity')!;
  const opacityValue = document.querySelector<HTMLSpanElement>('#opacity-value')!;
  const savedOpacity = localStorage.getItem('clipbridge-opacity') || '92';
  opacityInput.value = savedOpacity;
  opacityValue.textContent = `${savedOpacity}%`;
  opacityInput.oninput = () => {
    opacityValue.textContent = `${opacityInput.value}%`;
    localStorage.setItem('clipbridge-opacity', opacityInput.value);
    void emit('history-opacity-changed', Number(opacityInput.value) / 100);
  };
}
