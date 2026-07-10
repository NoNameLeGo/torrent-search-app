'use strict';

// ---- Size parsing -------------------------------------------------------
const SIZE_UNITS = {
  B: 1,
  KB: 1024, K: 1024, KIB: 1024,
  MB: 1024 ** 2, M: 1024 ** 2, MIB: 1024 ** 2,
  GB: 1024 ** 3, G: 1024 ** 3, GIB: 1024 ** 3,
  TB: 1024 ** 4, T: 1024 ** 4, TIB: 1024 ** 4,
  PB: 1024 ** 5, P: 1024 ** 5, PIB: 1024 ** 5,
};

// Parse strings like "1.2 GB", "800 MiB", "512 MB", "2048" -> bytes (number).
function parseSize(input) {
  if (input == null) return null;
  if (typeof input === 'number') return input;
  const s = String(input).trim().replace(/,/g, '');
  const m = s.match(/^([\d.]+)\s*([A-Za-z]+)?$/i);
  if (!m) return null;
  const num = parseFloat(m[1]);
  if (isNaN(num)) return null;
  const unit = (m[2] || 'B').toUpperCase();
  const mult = SIZE_UNITS[unit];
  if (!mult) return null;
  return Math.round(num * mult);
}

function formatSize(bytes) {
  if (bytes == null || isNaN(bytes)) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  let i = 0;
  let v = bytes;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v >= 100 || i === 0 ? 0 : 1)} ${units[i]}`;
}

// ---- Date parsing -------------------------------------------------------
// Handles "2 hours ago", "Yesterday", "3 days ago", "2024-01-02", etc.
function parseDate(input) {
  if (input == null) return null;
  if (typeof input === 'number') return input > 1e11 ? input : input * 1000; // unix s/ms
  const s = String(input).trim();
  if (!s) return null;

  // Relative: "X minutes/hours/days/weeks/months/years ago"
  const rel = s.match(/^(\d+)\s*(min|minute|hour|day|week|month|year)s?\s*ago$/i);
  if (rel) {
    const n = parseInt(rel[1], 10);
    const unit = rel[2].toLowerCase();
    const mult = {
      min: 60e3, minute: 60e3,
      hour: 3600e3, day: 86400e3,
      week: 604800e3, month: 2592000e3, year: 31536000e3,
    }[unit];
    return Date.now() - n * mult;
  }
  if (/^yesterday$/i.test(s)) return Date.now() - 86400e3;

  const t = Date.parse(s);
  return isNaN(t) ? null : t;
}

function formatDate(ts) {
  if (ts == null) return '—';
  const d = new Date(ts);
  if (isNaN(d.getTime())) return '—';
  return d.toISOString().slice(0, 10);
}

// ---- Magnet building ----------------------------------------------------
function buildMagnet(infoHash, name) {
  if (!infoHash) return null;
  const dn = name ? `&dn=${encodeURIComponent(name)}` : '';
  return `magnet:?xt=urn:btih:${infoHash}${dn}`;
}

function toInt(v) {
  if (v == null) return null;
  const n = parseInt(String(v).replace(/,/g, ''), 10);
  return isNaN(n) ? null : n;
}

// Normalize a raw provider result into the canonical shape used everywhere.
function normalize(raw) {
  const size = parseSize(raw.size);
  const dateTs = parseDate(raw.date);
  const magnet = raw.magnet || buildMagnet(raw.infoHash, raw.name);
  return {
    id: raw.id || `${raw.provider}:${raw.infoHash || raw.name}`,
    name: raw.name || '(untitled)',
    size,
    sizeText: formatSize(size),
    seeders: toInt(raw.seeders),
    leechers: toInt(raw.leechers),
    date: dateTs,
    dateText: formatDate(dateTs),
    magnet,
    infoHash: raw.infoHash || null,
    provider: raw.provider,
    category: raw.category || null,
    detailUrl: raw.detailUrl || null,
    files: toInt(raw.files),
    needsMagnet: !magnet && !!raw.detailUrl, // lazy-resolve on click
  };
}

module.exports = {
  parseSize, formatSize, parseDate, formatDate,
  buildMagnet, toInt, normalize,
};
