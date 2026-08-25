'use strict';

/**
 * Provider parser tests - using Node.js built-in assert.
 *
 * Run: node test/run.js
 */

const assert = require('assert');
const fs = require('fs');
const path = require('path');

// ---- Helpers -----------------------------------------------------------
function loadJSON(name) {
  return JSON.parse(fs.readFileSync(path.join(__dirname, 'fixtures', name), 'utf8'));
}

function loadHTML(name) {
  return fs.readFileSync(path.join(__dirname, 'fixtures', name), 'utf8');
}

// ---- Mock HTTP layer ---------------------------------------------------
// Creates a fresh mock for each test to avoid state leakage
function createMockHTTP(mockResponses) {
  // Clear require cache to force fresh module load
  delete require.cache[require.resolve('../src/lib/http')];
  for (const mod of ['../src/providers/tpb', '../src/providers/linuxtracker', '../src/providers/knaben']) {
    delete require.cache[require.resolve(mod)];
  }

  const httpModule = require('../src/lib/http');
  const originalGetJSON = httpModule.getJSON;
  const originalPostJSON = httpModule.postJSON;

  const matchResponse = (url) => {
    for (const [pattern, response] of mockResponses) {
      let matches = false;
      if (typeof pattern === 'function') {
        matches = pattern(url);
      } else if (pattern instanceof RegExp) {
        matches = pattern.test(url);
      } else {
        matches = url.startsWith(pattern);
      }
      if (matches) {
        return response;
      }
    }
    return null;
  };

  httpModule.getJSON = async (url) => matchResponse(url) ?? originalGetJSON(url);
  httpModule.postJSON = async (url) => matchResponse(url) ?? originalPostJSON(url);

  return () => {
    httpModule.getJSON = originalGetJSON;
    httpModule.postJSON = originalPostJSON;
    // Clear cache again
    delete require.cache[require.resolve('../src/lib/http')];
    for (const mod of ['../src/providers/tpb', '../src/providers/linuxtracker', '../src/providers/knaben']) {
      delete require.cache[require.resolve(mod)];
    }
  };
}

function createMockHTTPForHTML(mockResponses) {
  // Clear require cache
  delete require.cache[require.resolve('../src/lib/http')];
  for (const mod of ['../src/providers/linuxtracker', '../src/providers/filemood']) {
    delete require.cache[require.resolve(mod)];
  }

  const httpModule = require('../src/lib/http');
  const originalGetText = httpModule.getText;

  httpModule.getText = async function(url) {
    for (const [pattern, response] of mockResponses) {
      let matches = false;
      if (typeof pattern === 'function') {
        matches = pattern(url);
      } else if (pattern instanceof RegExp) {
        matches = pattern.test(url);
      } else {
        matches = url.startsWith(pattern);
      }
      if (matches) {
        return response;
      }
    }
    return originalGetText(url);
  };

  return () => {
    httpModule.getText = originalGetText;
    delete require.cache[require.resolve('../src/lib/http')];
    for (const mod of ['../src/providers/linuxtracker', '../src/providers/filemood']) {
      delete require.cache[require.resolve(mod)];
    }
  };
}

// ---- Tests ------------------------------------------------------------
console.log('Running provider tests...\n');

// ============================================================================
// TPB (The Pirate Bay) Tests
// ============================================================================
console.log('=== TPB (The Pirate Bay) ===\n');

(async () => {
  // Test 1: Basic search with real fixture data
  console.log('Test 1: Parse real TPB results...');
  const fixture = loadJSON('tpb-ubuntu.json');
  const cleanup1 = createMockHTTP([
    [/(?:^https?:)?\/\/apibay\.org/i, { data: fixture, error: null }]
  ]);

  const { search: tpbSearch } = require('../src/providers/tpb');
  const result = await tpbSearch('ubuntu', { page: 1 });
  cleanup1();

  assert.ok(!result.error, 'should not have error');
  assert.ok(Array.isArray(result.results), 'results should be an array');
  assert.ok(result.results.length > 0, 'should have at least one result');
  console.log(`  ✓ Found ${result.results.length} results\n`);

  // Check first result shape
  const first = result.results[0];
  assert.ok(typeof first.id === 'string', 'id should be string');
  assert.ok(typeof first.name === 'string', 'name should be string');
  assert.equal(first.provider, 'tpb', 'provider should be tpb');
  assert.ok(first.infoHash, 'should have infoHash');
  assert.ok(first.magnet?.startsWith('magnet:?'), 'should have magnet URI');
  assert.ok(typeof first.size === 'number', 'size should be number (bytes)');
  assert.ok(typeof first.seeders === 'number', 'seeders should be number');
  assert.ok(typeof first.category === 'string', 'category should be string');
  console.log('  ✓ Result shape is correct\n');

  // Test 2: Category mapping
  console.log('Test 2: Category mapping...');
  const categories = new Set(result.results.map(r => r.category));
  console.log('  Categories found:', [...categories]);
  assert.ok(categories.has('Apps') || categories.has('Books'), 'should have valid categories');
  console.log('  ✓ Category mapping works correctly\n');

  // Test 3: Empty results
  console.log('Test 3: Empty results...');
  const cleanup3 = createMockHTTP([
    [/(?:^https?:)?\/\/apibay\.org/i, { data: [{ error: 'no results' }], error: null }]
  ]);
  const { search: tpbSearchEmpty } = require('../src/providers/tpb');
  const emptyResult = await tpbSearchEmpty('nonexistent-query-xyz-12345', { page: 1 });
  cleanup3();

  assert.equal(emptyResult.results.length, 0, 'should return empty results array');
  assert.equal(emptyResult.error, null, 'should not have error for empty results');
  console.log('  ✓ Empty results handled correctly\n');

  // Test 4: HTTP error
  console.log('Test 4: HTTP error handling...');
  const cleanup4 = createMockHTTP([
    [/(?:^https?:)?\/\/apibay\.org/i, { data: null, error: 'ECONNRESET' }]
  ]);
  const { search: tpbSearchError } = require('../src/providers/tpb');
  const errorResult = await tpbSearchError('test', { page: 1 });
  cleanup4();

  assert.equal(errorResult.results.length, 0, 'should return empty results on error');
  assert.ok(errorResult.error?.includes('TPB unreachable'), 'should include error message');
  console.log('  ✓ HTTP error handled correctly\n');
})();

// ============================================================================
// Knaben Tests (JSON API provider, POST)
// ============================================================================
console.log('=== Knaben ===\n');

(async () => {
  console.log('Test 1: Parse real Knaben JSON...');
  const fixture = loadJSON('knaben-ubuntu.json');
  const cleanup = createMockHTTP([
    [/(?:^https?:)?\/\/api\.knaben\.org/i, { data: fixture, error: null }]
  ]);

  const { search } = require('../src/providers/knaben');
  const result = await search('ubuntu', { page: 1 });
  cleanup();

  assert.ok(!result.error, 'should not have error');
  assert.ok(Array.isArray(result.results), 'results should be an array');
  assert.ok(result.results.length > 0, 'should have at least one result');
  console.log(`  ✓ Found ${result.results.length} results\n`);

  const first = result.results[0];
  assert.ok(typeof first.name === 'string' && first.name.length > 0, 'name should be non-empty');
  assert.equal(first.provider, 'knaben', 'provider should be knaben');
  assert.ok(first.infoHash, 'should have infoHash');
  assert.ok(first.magnet?.startsWith('magnet:?'), 'should have magnet URI');
  assert.ok(typeof first.size === 'number', 'size should be parsed');
  assert.ok(typeof first.seeders === 'number', 'seeders should be parsed');
  assert.equal(typeof first.category, 'string', 'category should be string');
  console.log('  First result:', first.name, '-', first.sizeText, '-', first.seeders, 'seeds -', first.category);
  console.log('  ✓ Result shape is correct\n');
})();

// ============================================================================
// LinuxTracker Tests (HTML-based provider)
// ============================================================================
console.log('=== LinuxTracker ===\n');

(async () => {
  // Test 1: Parse real HTML fixture
  console.log('Test 1: Parse LinuxTracker HTML...');
  const html = loadHTML('linuxtracker-linux.html');
  const cleanup = createMockHTTPForHTML([
    [/(?:^https?:)?\/\/linuxtracker\.org/i, { html, error: null }]
  ]);

  const { search } = require('../src/providers/linuxtracker');
  const result = await search('linux', { page: 1 });
  cleanup();

  assert.ok(!result.error, 'should not have error');
  assert.ok(Array.isArray(result.results), 'results should be an array');
  assert.ok(result.results.length > 0, 'should have at least one result');
  console.log(`  ✓ Found ${result.results.length} results\n`);

  // Check first result shape
  const first = result.results[0];
  assert.ok(typeof first.name === 'string', 'name should be string');
  assert.ok(first.name.length > 0, 'name should not be empty');
  assert.equal(first.provider, 'linuxtracker', 'provider should be linuxtracker');
  assert.ok(first.infoHash, 'should have infoHash from URL');
  assert.ok(first.magnet?.startsWith('magnet:?'), 'should have magnet URI');
  assert.ok(typeof first.size === 'number', 'size should be parsed');
  assert.ok(typeof first.seeders === 'number', 'seeders should be parsed');
  console.log('  First result:', first.name, '-', first.sizeText, '-', first.seeders, 'seeds');
  console.log('  ✓ Result shape is correct\n');
})();

// ============================================================================
// FileMood Tests (HTML-based provider, single-domain)
// ============================================================================
console.log('=== FileMood ===\n');

(async () => {
  console.log('Test 1: Parse real FileMood HTML...');
  const html = loadHTML('filemood-ubuntu.html');
  const cleanup = createMockHTTPForHTML([
    [/(?:^https?:)?\/\/filemood\.com/i, { html, error: null }]
  ]);

  const { search } = require('../src/providers/filemood');
  const result = await search('ubuntu', { page: 1 });
  cleanup();

  assert.ok(!result.error, 'should not have error');
  assert.ok(Array.isArray(result.results), 'results should be an array');
  assert.ok(result.results.length > 0, 'should have at least one result');
  console.log(`  ✓ Found ${result.results.length} results\n`);

  const first = result.results[0];
  assert.ok(typeof first.name === 'string' && first.name.length > 0, 'name should be non-empty');
  assert.equal(first.provider, 'filemood', 'provider should be filemood');
  assert.ok(first.infoHash, 'should have infoHash from detail URL');
  assert.match(first.infoHash, /^[a-f0-9]{40}$/, 'infoHash should be 40 hex chars');
  assert.ok(first.magnet?.startsWith('magnet:?'), 'should have magnet URI');
  assert.ok(typeof first.size === 'number', 'size should be parsed');
  assert.ok(typeof first.seeders === 'number', 'seeders should be parsed');
  console.log('  First result:', first.name, '-', first.sizeText, '-', first.seeders, 'seeds');
  console.log('  ✓ Result shape is correct\n');
})();

// ============================================================================
// Normalize Utility Tests (synchronous, no network needed)
// ============================================================================
console.log('=== Normalize Utility ===\n');

const { normalize, parseSize, parseDate, buildMagnet, extractInfoHash } = require('../src/lib/normalize');

// Size parsing
console.log('Testing parseSize...');
assert.equal(parseSize('1.2 GB'), Math.round(1.2 * 1024 ** 3));
assert.equal(parseSize('800 MiB'), Math.round(800 * 1024 ** 2));
assert.equal(parseSize('512 MB'), 512 * 1024 ** 2);
assert.equal(parseSize('2048'), 2048);
assert.equal(parseSize(null), null);
assert.equal(parseSize(undefined), null);
console.log('✓ parseSize works correctly\n');

// Date parsing
console.log('Testing parseDate...');
const ts = Math.floor(Date.now() / 1000);
assert.equal(parseDate(ts), ts * 1000);

const ms = Date.now();
assert.equal(parseDate(ms), ms);

const now = Date.now();
assert.ok(parseDate('2 hours ago') <= now);
assert.ok(parseDate('3 days ago') < parseDate('2 hours ago'));
assert.ok(parseDate('yesterday') < parseDate('today'));
console.log('✓ parseDate works correctly\n');

// Magnet building
console.log('Testing buildMagnet...');
const magnet = buildMagnet('ABC123', 'Test Torrent');
assert.ok(magnet.startsWith('magnet:?xt=urn:btih:ABC123&dn=Test%20Torrent'), 'magnet should have correct format');

const magnetNoName = buildMagnet('ABC123');
assert.ok(magnetNoName.startsWith('magnet:?xt=urn:btih:ABC123'), 'magnet should work without name');

assert.equal(buildMagnet(null), null);
console.log('✓ buildMagnet works correctly\n');

// InfoHash extraction
console.log('Testing extractInfoHash...');
const hash = extractInfoHash('magnet:?xt=urn:btih:ABC123DEF456789012345678901234567890&dn=test');
assert.equal(hash, 'abc123def456789012345678901234567890'); // normalized to lowercase

assert.equal(extractInfoHash('not a magnet'), null);
assert.equal(extractInfoHash(null), null);
console.log('✓ extractInfoHash works correctly\n');

// Full normalization
console.log('Testing normalize...');
const raw = {
  provider: 'tpb',
  name: 'Test Torrent',
  infoHash: 'ABC123',
  size: '1.5 GB',
  seeders: 100,
  leechers: 10,
  date: 1609459200, // 2021-01-01
  category: 'Apps',
  detailUrl: 'https://example.com/torrent/123',
};

const normalized = normalize(raw);

assert.equal(normalized.id, 'tpb:ABC123');
assert.equal(normalized.name, 'Test Torrent');
assert.equal(normalized.provider, 'tpb');
assert.equal(normalized.size, Math.round(1.5 * 1024 ** 3));
assert.equal(normalized.sizeText, '1.5 GB');
assert.equal(normalized.seeders, 100);
assert.equal(normalized.leechers, 10);
assert.equal(normalized.date, 1609459200000);
assert.equal(normalized.category, 'Apps');
assert.equal(normalized.detailUrl, 'https://example.com/torrent/123');
assert.ok(normalized.magnet?.startsWith('magnet:?'));
assert.equal(normalized.needsMagnet, false); // has infoHash, so magnet is built
console.log('✓ normalize works correctly\n');

// Needs magnet case
console.log('Testing needsMagnet flag...');
const rawNoHash = {
  provider: '1337x',
  name: 'Test Torrent',
  size: '500 MB',
  detailUrl: 'https://1337x.to/torrent/123/',
};

const normalizedNoHash = normalize(rawNoHash);
assert.equal(normalizedNoHash.needsMagnet, true);
assert.equal(normalizedNoHash.magnet, null);
console.log('✓ needsMagnet flag works correctly\n');

console.log('════════════════════════════════════════');
console.log('All tests passed! ✓');
console.log('════════════════════════════════════════');
