'use strict';

import { state, $, HISTORY_MAX } from './state.js';
import { esc } from './utils.js';

export function pushHistory(q) {
  const v = q.trim();
  if (!v) return;
  state.history = [v, ...state.history.filter((h) => h.toLowerCase() !== v.toLowerCase())]
    .slice(0, HISTORY_MAX);
  localStorage.setItem('history', JSON.stringify(state.history));
}

export function removeHistory(q) {
  state.history = state.history.filter((h) => h !== q);
  localStorage.setItem('history', JSON.stringify(state.history));
  renderHistory();
}

export function clearHistory() {
  state.history = [];
  localStorage.setItem('history', JSON.stringify(state.history));
  hideHistory();
}

export function renderHistory() {
  const box = $('#history-dropdown');
  if (!state.history.length) { box.hidden = true; box.innerHTML = ''; return; }
  const head =
    `<div class="history-head"><span>最近搜索</span>` +
    `<button class="history-clear" data-clear="1">清空全部</button></div>`;
  const items = state.history.map((h) =>
    `<div class="history-item">` +
    `<button type="button" class="history-term" data-q="${esc(h)}">` +
    `<svg class="h-icon" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>` +
    `<span class="h-term">${esc(h)}</span></button>` +
    `<button type="button" class="h-del" data-del="${esc(h)}" title="删除" aria-label="删除 ${esc(h)}">✕</button>` +
    `</div>`
  ).join('');
  box.innerHTML = head + items;
  box.hidden = false;
}

export function showHistory() { if (state.history.length) renderHistory(); }
export function hideHistory() { $('#history-dropdown').hidden = true; }