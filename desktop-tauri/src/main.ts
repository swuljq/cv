import { invoke } from '@tauri-apps/api/core';
import './styles.css';

const app = document.querySelector<HTMLDivElement>('#app')!;
app.innerHTML = `<main><h2>ClipBridge</h2><input id="server" value="ws://104.233.216.159:8787" placeholder="服务器地址"><input id="user" value="clipbridge" placeholder="账号"><input id="password" value="JocK36vOBqd4" type="password" placeholder="密码"><button id="connect">连接并开始同步</button><p id="status">未连接</p><small>连接后可最小化到后台，原生同步服务会继续运行。</small></main>`;
const $ = (id: string) => document.querySelector<HTMLInputElement | HTMLButtonElement | HTMLParagraphElement>(`#${id}`)!;
($('connect') as HTMLButtonElement).onclick = async () => {
  $('status').textContent = '连接中…';
  try { await invoke('start_sync', { url: ($('server') as HTMLInputElement).value, username: ($('user') as HTMLInputElement).value, password: ($('password') as HTMLInputElement).value }); $('status').textContent = '已连接，后台同步中'; }
  catch (error) { $('status').textContent = `连接失败：${error}`; }
};
