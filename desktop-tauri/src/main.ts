import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import './styles.css';

const app = document.querySelector<HTMLDivElement>('#app')!;
app.innerHTML = `<main><h2>ClipBridge</h2><input id="server" value="ws://104.233.216.159:8787" placeholder="服务器地址"><input id="user" value="clipbridge" placeholder="账号"><input id="password" value="JocK36vOBqd4" type="password" placeholder="密码"><button id="connect">连接并开始同步</button><p id="status">未连接</p><small>连接后可最小化到后台，按 Win+V 查看最近复制的 10 条内容；若被系统占用则按 Ctrl+Shift+V。</small></main><section id="history" hidden><div class="history-card"><div class="history-head"><strong>最近复制</strong><button id="close-history">关闭</button></div><div id="history-list"></div><label class="opacity-control">透明度 <input id="opacity" type="range" min="50" max="100" value="92"><span id="opacity-value">92%</span></label></div></section>`;
const $ = (id: string) => document.querySelector<HTMLInputElement | HTMLButtonElement | HTMLParagraphElement>(`#${id}`)!;
($('connect') as HTMLButtonElement).onclick = async () => {
  $('status').textContent = '连接中…';
  try { await invoke('start_sync', { url: ($('server') as HTMLInputElement).value, username: ($('user') as HTMLInputElement).value, password: ($('password') as HTMLInputElement).value }); $('status').textContent = '已连接，后台同步中'; }
  catch (error) { $('status').textContent = `连接失败：${error}`; }
};

const historyPanel = document.querySelector<HTMLElement>('#history')!;
const historyList = document.querySelector<HTMLElement>('#history-list')!;
const renderHistory = async () => {
  const items = await invoke<string[]>('get_clipboard_history');
  const pinned = new Set(JSON.parse(localStorage.getItem('clipbridge-pinned') || '[]') as string[]);
  const ordered = [...items].sort((a, b) => Number(pinned.has(b)) - Number(pinned.has(a)));
  historyList.innerHTML = ordered.length ? ordered.map((item, index) => `<div class="history-row"><button class="history-item" data-index="${index}">${escapeHtml(item)}</button><button class="pin-item" data-value="${encodeURIComponent(item)}" title="置顶">${pinned.has(item) ? '★' : '☆'}</button></div>`).join('') : '<p class="empty">还没有复制记录</p>';
  historyList.querySelectorAll<HTMLButtonElement>('.history-item').forEach(button => {
    button.onclick = async () => {
      const item = ordered[Number(button.dataset.index)];
      await invoke('select_clipboard_history', { value: item });
      historyPanel.hidden = true;
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
};
const escapeHtml = (value: string) => value.replace(/[&<>'"]/g, character => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', "'": '&#39;', '"': '&quot;' }[character] || character));
document.querySelector<HTMLButtonElement>('#close-history')!.onclick = () => { historyPanel.hidden = true; };
const opacityInput = document.querySelector<HTMLInputElement>('#opacity')!;
const opacityValue = document.querySelector<HTMLSpanElement>('#opacity-value')!;
const savedOpacity = localStorage.getItem('clipbridge-opacity') || '92';
opacityInput.value = savedOpacity;
historyPanel.style.setProperty('--history-opacity', `${Number(savedOpacity) / 100}`);
opacityValue.textContent = `${savedOpacity}%`;
opacityInput.oninput = () => {
  historyPanel.style.setProperty('--history-opacity', `${Number(opacityInput.value) / 100}`);
  opacityValue.textContent = `${opacityInput.value}%`;
  localStorage.setItem('clipbridge-opacity', opacityInput.value);
};
await listen('clipboard-history-open', async () => { await renderHistory(); historyPanel.hidden = false; });
