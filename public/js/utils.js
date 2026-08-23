'use strict';

import { state } from './state.js';

// ---------- HTML escaping ----------
export function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

// ---------- keyword highlighting ----------
export function highlight(name, query) {
  const safe = esc(name);
  const q = String(query || '').trim();
  if (!q) return safe;
  const tokens = [...new Set(q.split(/\s+/).filter((t) => t.length >= 1))]
    .map(esc)
    .filter(Boolean)
    .sort((a, b) => b.length - a.length)
    .map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  if (!tokens.length) return safe;
  const re = new RegExp(`(${tokens.join('|')})`, 'gi');
  return safe.replace(re, '<span class="hl">$1</span>');
}

// ---------- magnet truncation ----------
export function shortMagnet(m, n = 56) {
  const s = String(m || '');
  return s.length > n ? `${s.slice(0, n)}…` : s;
}

// ---------- seed health bar ----------
export function seedBarHTML(it) {
  const s = it.seeders, l = it.leechers;
  if ((s == null || s === 0) && (l == null || l === 0)) return '';
  const total = (s || 0) + (l || 0);
  if (total === 0) return '';
  const pct = Math.round((s || 0) / total * 100);
  const color = pct > 66 ? 'var(--green)' : pct > 33 ? 'var(--yellow)' : 'var(--red)';
  return `<div class="seed-bar"><div class="seed-fill" style="width:${pct}%;background:${color}"></div></div>`;
}

// ---------- relevance scoring ----------
export function relevanceScore(name, query) {
  const n = String(name || '').toLowerCase();
  const q = String(query || '').toLowerCase().trim();
  if (!q) return 0;
  let score = 0;
  if (n.includes(q)) score += 5;
  if (n.startsWith(q)) score += 3;
  const split = (s) => s.split(/[^a-z0-9一-鿿]+/).filter(Boolean);
  const nTokens = split(n);
  const qTokens = split(q);
  const nSet = new Set(nTokens);
  let hit = 0;
  for (const t of qTokens) {
    if (nSet.has(t)) { score += 2; hit++; }
    else if (nTokens.some((w) => w.startsWith(t))) { score += 1; hit++; }
    else if (n.includes(t)) { score += 0.5; hit++; }
  }
  if (qTokens.length) score += (hit / qTokens.length) * 3;
  return score;
}

// ---------- toast ----------
let toastTimer;
export function toast(msg) {
  const t = document.querySelector('#toast');
  if (!t) return;
  t.textContent = msg; t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { t.hidden = true; }, 2200);
}

// ---------- clipboard ----------
export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    toast('磁力链接已复制');
  } catch {
    const ta = document.createElement('textarea');
    ta.value = text; document.body.appendChild(ta); ta.select();
    try { document.execCommand('copy'); toast('磁力链接已复制'); }
    catch { toast('复制失败，请手动复制'); }
    document.body.removeChild(ta);
  }
}

// ---------- CSV cell escaping ----------
export function csvCell(v) {
  let s = v == null ? '' : String(v);
  if (/^[=+\-@\t\r]/.test(s)) s = "'" + s;
  return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}