'use strict';

import { state, $, DL_CLIENTS, allProviders } from './state.js';
import { esc, toast } from './utils.js';
import { render } from './render.js';

// ---------- download client settings ----------
export function syncDlAuthFields() {
  const kind = $('#dl-client').value;
  const meta = DL_CLIENTS[kind] || DL_CLIENTS.qbittorrent;
  $('#dl-userpass').hidden = meta.auth !== 'userpass';
  $('#dl-tokenwrap').hidden = meta.auth !== 'token';
  $('#dl-token-label').textContent = kind === 'aria2'
    ? 'RPC 密钥（rpc-secret，无则留空）'
    : 'API Token（无则留空）';
}

export function openSettings() {
  const dl = state.dl || {};
  $('#dl-client').value = dl.client || 'qbittorrent';
  $('#dl-url').value = dl.url || '';
  $('#dl-user').value = dl.user || '';
  $('#dl-pass').value = dl.pass || '';
  $('#dl-token').value = dl.token || '';
  syncDlAuthFields();
  $('#settings-modal').hidden = false;
}

export function closeSettings() { $('#settings-modal').hidden = true; }

export function readDlForm() {
  return {
    client: $('#dl-client').value,
    url: $('#dl-url').value.trim(),
    user: $('#dl-user').value.trim(),
    pass: $('#dl-pass').value,
    token: $('#dl-token').value.trim(),
  };
}

// ---------- torznab management ----------
export async function loadTorznab() {
  const wrap = $('#torznab-list');
  if (!wrap) return;
  try {
    const r = await fetch('/api/torznab');
    const { indexers } = await r.json();
    wrap.innerHTML = '';
    (indexers || []).forEach((it) => {
      const row = document.createElement('div');
      row.className = 'tn-item';
      row.innerHTML =
        `<span class="tn-name">${esc(it.name)}</span>` +
        `<span class="tn-url">${esc(it.url)}</span>` +
        `<button class="btn ghost tn-del" data-id="${esc(it.id)}">删除</button>`;
      wrap.appendChild(row);
    });
  } catch (e) { /* ignore */ }
}