'use strict';

/**
 * Normalize utility tests - golden file tests for parseSize, parseDate, etc.
 *
 * Run: node test/normalize.test.js
 */

const assert = require('assert');
const { normalize, parseSize, parseDate, buildMagnet, extractInfoHash, ruDate } = require('../src/lib/normalize');

console.log('Running normalize tests...\n');

// ============================================================================
// parseSize Tests
// ============================================================================
console.log('=== parseSize ===\n');

assert.equal(parseSize('1.2 GB'), Math.round(1.2 * 1024 ** 3), 'GB');
assert.equal(parseSize('800 MiB'), Math.round(800 * 1024 ** 2), 'MiB');
assert.equal(parseSize('512 MB'), 512 * 1024 ** 2, 'MB');
assert.equal(parseSize('1 TB'), 1024 ** 4, 'TB');
assert.equal(parseSize('1 PB'), 1024 ** 5, 'PB');
assert.equal(parseSize('1 KB'), 1024, 'KB');
assert.equal(parseSize('1 K'), 1024, 'K alias');
assert.equal(parseSize('1 KIB'), 1024, 'KIB alias');
assert.equal(parseSize('2048'), 2048, 'bare bytes');
assert.equal(parseSize('1.5G'), Math.round(1.5 * 1024 ** 3), 'G shorthand');
assert.equal(parseSize('1.5G'), parseSize('1.5 GB'), 'consistent with space');
assert.equal(parseSize('1,024 MB'), 1024 * 1024 ** 2, 'comma separator');
assert.equal(parseSize(null), null, 'null');
assert.equal(parseSize(undefined), null, 'undefined');
assert.equal(parseSize(''), null, 'empty string');
assert.equal(parseSize('abc'), null, 'invalid string');
assert.equal(parseSize(12345), 12345, 'numeric input');

console.log('✓ All parseSize tests passed\n');

// ============================================================================
// parseDate Tests
// ============================================================================
console.log('=== parseDate ===\n');

// Unix timestamps (capture once to avoid timing issues)
const unixSeconds = Math.floor(Date.now() / 1000);
const unixMs = unixSeconds * 1000;

assert.equal(parseDate(unixSeconds), unixMs, 'unix seconds');
assert.equal(parseDate(unixMs), unixMs, 'unix milliseconds');

// Relative dates (use a fixed reference point to avoid timing issues)
const refTime = Date.now();
assert.ok(parseDate('2 hours ago') <= refTime, 'relative: 2 hours ago');
assert.ok(parseDate('3 days ago') < parseDate('2 hours ago'), 'ordering: days < hours');
assert.ok(parseDate('yesterday') < parseDate('today'), 'ordering: yesterday < today');
assert.ok(parseDate('last month') < parseDate('yesterday'), 'ordering: last month < yesterday');

// Special phrases
assert.ok(parseDate('a minute ago') <= refTime, 'a minute ago');
assert.ok(parseDate('an hour ago') <= refTime, 'an hour ago');
assert.ok(parseDate('a day ago') <= refTime, 'a day ago');

// Date strings
const dateTs = Date.parse('2024-01-15');
assert.equal(parseDate('2024-01-15'), dateTs, 'ISO date');
// EU date format not supported by Date.parse in all environments, skip strict check

// Null/invalid
assert.equal(parseDate(null), null, 'null');
assert.equal(parseDate(undefined), null, 'undefined');
assert.equal(parseDate(''), null, 'empty string');
assert.equal(parseDate('not a date'), null, 'invalid string');

console.log('✓ All parseDate tests passed\n');

// ============================================================================
// buildMagnet Tests
// ============================================================================
console.log('=== buildMagnet ===\n');

assert.ok(buildMagnet('ABC123', 'Test Torrent').startsWith('magnet:?xt=urn:btih:ABC123&dn='), 'with name');
assert.ok(buildMagnet('ABC123').startsWith('magnet:?xt=urn:btih:ABC123'), 'without name');
assert.equal(buildMagnet(null), null, 'null hash');
assert.equal(buildMagnet(''), null, 'empty hash');
assert.equal(buildMagnet(), null, 'no args');

console.log('✓ All buildMagnet tests passed\n');

// ============================================================================
// extractInfoHash Tests
// ============================================================================
console.log('=== extractInfoHash ===\n');

// Valid hashes (32-40 hex chars)
assert.equal(
  extractInfoHash('magnet:?xt=urn:btih:ABC123DEF456789012345678901234567890&dn=test'),
  'abc123def456789012345678901234567890',
  'valid hash normalized to lowercase'
);

assert.equal(
  extractInfoHash('btih:ABC123DEF456789012345678901234567890'),
  'abc123def456789012345678901234567890',
  'bare btih string'
);

// Invalid cases
assert.equal(extractInfoHash('not a magnet'), null, 'no btih');
assert.equal(extractInfoHash(null), null, 'null');
assert.equal(extractInfoHash(''), null, 'empty');
assert.equal(extractInfoHash('short'), null, 'too short');

console.log('✓ All extractInfoHash tests passed\n');

// ============================================================================
// ruDate Tests
// ============================================================================
console.log('=== ruDate ===\n');

assert.ok(ruDate('Сегодня').includes('Today'), 'today translation');
assert.ok(ruDate('Вчера').includes('Yesterday'), 'yesterday translation');
// ruDate converts Russian months to English for Date.parse compatibility
assert.ok(ruDate('мая').includes('May'), 'genitive month -> May');
assert.ok(ruDate('янв').includes('Jan'), 'Russian month -> Jan');
assert.ok(ruDate('Сегодня').includes('Today'), 'today translation');
assert.ok(ruDate('Вчера').includes('Yesterday'), 'yesterday translation');

console.log('✓ All ruDate tests passed\n');

// ============================================================================
// normalize() Full Integration Tests
// ============================================================================
console.log('=== normalize() ===\n');

// Full result with all fields
const fullRaw = {
  provider: 'tpb',
  name: 'Ubuntu 22.04 LTS',
  infoHash: '2C6B6858D61DA9543D4231A71DB4B1C9264B0685',
  size: '3.4 GB',
  seeders: 39,
  leechers: 1,
  date: 1652877231,
  category: 'Apps',
  detailUrl: 'https://thepiratebay.org/description.php?id=59191690',
  files: 1,
};

const fullResult = normalize(fullRaw);

assert.equal(fullResult.id, 'tpb:2C6B6858D61DA9543D4231A71DB4B1C9264B0685', 'id format');
assert.equal(fullResult.name, 'Ubuntu 22.04 LTS', 'name');
assert.equal(fullResult.provider, 'tpb', 'provider');
assert.equal(fullResult.size, Math.round(3.4 * 1024 ** 3), 'size in bytes');
assert.equal(fullResult.sizeText, '3.4 GB', 'size text');
assert.equal(fullResult.seeders, 39, 'seeders');
assert.equal(fullResult.leechers, 1, 'leechers');
assert.equal(fullResult.date, 1652877231000, 'date as ms');
assert.equal(fullResult.category, 'Apps', 'category');
assert.equal(fullResult.detailUrl, 'https://thepiratebay.org/description.php?id=59191690', 'detailUrl');
assert.equal(fullResult.files, 1, 'files');
assert.ok(fullResult.magnet?.startsWith('magnet:?'), 'magnet URI');
assert.equal(fullResult.needsMagnet, false, 'no need for magnet');

// Minimal result (only required fields)
const minimalRaw = {
  provider: 'demo',
  name: 'Test',
};

const minimalResult = normalize(minimalRaw);
assert.equal(minimalResult.id, 'demo:Test', 'id from provider:name');
assert.equal(minimalResult.name, 'Test', 'name');
assert.equal(minimalResult.provider, 'demo', 'provider');
assert.equal(minimalResult.size, null, 'size null');
assert.equal(minimalResult.seeders, null, 'seeders null');
assert.equal(minimalResult.magnet, null, 'magnet null');
assert.equal(minimalResult.needsMagnet, false, 'no detailUrl so no needsMagnet');

// Result needing magnet (has detailUrl but no infoHash)
const needsMagnetRaw = {
  provider: '1337x',
  name: 'Ubuntu ISO',
  size: '2 GB',
  detailUrl: 'https://1337x.to/torrent/123/',
};

const needsMagnetResult = normalize(needsMagnetRaw);
assert.equal(needsMagnetResult.needsMagnet, true, 'needs magnet when no infoHash');
assert.equal(needsMagnetResult.magnet, null, 'magnet is null');

console.log('✓ All normalize() tests passed\n');

// ============================================================================
// Edge Cases
// ============================================================================
console.log('=== Edge Cases ===\n');

// Size edge cases
assert.equal(parseSize('0 B'), 0, 'zero size');
assert.equal(parseSize('0'), 0, 'zero bytes');
assert.ok(parseSize('1 PB') > parseSize('1 TB'), 'PB > TB');

// Date edge cases
assert.ok(parseDate('1 year ago') < parseDate('1 month ago'), 'year > month');
assert.ok(parseDate('1 week ago') < parseDate('1 day ago'), 'week > day');

// Magnet edge cases
assert.ok(buildMagnet('A'.repeat(40), 'Test')?.length > 50, 'long hash');
assert.ok(buildMagnet('a'.repeat(32), 'Test')?.length > 50, 'short hash (32 chars)');

console.log('✓ All edge case tests passed\n');

console.log('════════════════════════════════════════');
console.log('All normalize tests passed! ✓');
console.log('════════════════════════════════════════');
