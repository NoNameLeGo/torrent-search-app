'use strict';

/**
 * Golden-file tests for provider parsers.
 *
 * Strategy: mock the HTTP layer to return saved fixture data,
 * then assert the parsed results match expected shapes.
 * This avoids network flakiness and site redesigns breaking tests.
 *
 * Run: node --test test/providers.test.js
 *       node --test test/providers.test.js --coverage
 */

const { describe, it } = require('node:test');
const { equal, deepEqual, ok, match } = require('node:assert');
const fs = require('fs');
const path = require('path');

// ---- Fixtures -----------------------------------------------------------
const FIXTURES_DIR = path.resolve(__dirname, 'fixtures');

function loadJSON(name) {
  return JSON.parse(fs.readFileSync(path.join(FIXTURES_DIR, name), 'utf8'));
}

function loadText(name) {
  return fs.readFileSync(path.join(FIXTURES_DIR, name), 'utf8');
}

// ---- Mock HTTP layer ----------------------------------------------------
// We mock http.js so providers use our fixture data instead of real networks.
function mockHTTP(mockResponses) {
  // mockResponses: Map<urlPattern, { data, error }>
  // urlPattern can be a string prefix or a function(url) => boolean
  const originalGetJSON = require('../src/lib/http').getJSON;

  // Monkey-patch getJSON
  require('../src/lib/http').getJSON = async function(url) {
    for (const [pattern, response] of mockResponses) {
      const matches = typeof pattern === 'function' ? pattern(url) : url.startsWith(pattern);
      if (matches) {
        return response;
      }
    }
    return originalGetJSON(url);
  };

  // Restore after test
  return () => { require('../src/lib/http').getJSON = originalGetJSON; };
}

// ---- Fixtures -----------------------------------------------------------
// tpb-ubuntu.json: real API response from apibay.org/q.php?q=ubuntu
// Contains ~10 results with mixed categories (Apps, Books, Other)

// ---- Tests ------------------------------------------------------------
describe('tpb (The Pirate Bay)', () => {
  let cleanup;

  it('parses ubuntu search results correctly', async () => {
    const fixture = loadJSON('tpb-ubuntu.json');
    cleanup = mockHTTP([
      [/(?:^https?:)?\/\/apibay\.org/i, { data: fixture, error: null }]
    ]);

    const { search } = require('../src/providers/tpb');
    const result = await search('ubuntu', { page: 1 });

    cleanup();

    ok(!result.error, 'should not have error');
    ok(Array.isArray(result.results), 'results should be an array');
    ok(result.results.length > 0, 'should have at least one result');

    // Check first result shape
    const first = result.results[0];
    equal(typeof first.id, 'string', 'id should be string');
    equal(typeof first.name, 'string', 'name should be string');
    equal(first.provider, 'tpb', 'provider should be tpb');
    ok(first.infoHash, 'should have infoHash');
    ok(first.magnet?.startsWith('magnet:?'), 'should have magnet URI');
    ok(typeof first.size === 'number', 'size should be number (bytes)');
    ok(typeof first.seeders === 'number', 'seeders should be number');
    equal(typeof first.category, 'string', 'category should be string');
  });

  it('maps TPB category codes correctly', async () => {
    const fixture = loadJSON('tpb-ubuntu.json');
    cleanup = mockHTTP([
      [/(?:^https?:)?\/\/apibay\.org/i, { data: fixture, error: null }]
    ]);

    const { search } = require('../src/providers/tpb');
    const result = await search('ubuntu', { page: 1 });
    cleanup();

    const categories = new Set(result.results.map(r => r.category));
    // Ubuntu results should include Apps and Books categories
    ok(categories.has('Apps') || categories.has('Books'), 'should have valid categories');
  });

  it('handles empty results', async () => {
    // Simulate apibay "no results" response
    const emptyResponse = [{ error: 'no results' }];
    cleanup = mockHTTP([
      [/(?:^https?:)?\/\/apibay\.org/i, { data: emptyResponse, error: null }]
    ]);

    const { search } = require('../src/providers/tpb');
    const result = await search('nonexistent-query-xyz-12345', { page: 1 });
    cleanup();

    equal(result.results.length, 0, 'should return empty results array');
    equal(result.error, null, 'should not have error for empty results');
  });

  it('handles HTTP error', async () => {
    cleanup = mockHTTP([
      [/(?:^https?:)?\/\/apibay\.org/i, { data: null, error: 'ECONNRESET' }]
    ]);

    const { search } = require('../src/providers/tpb');
    const result = await search('test', { page: 1 });
    cleanup();

    equal(result.results.length, 0, 'should return empty results on error');
    ok(result.error?.includes('TPB unreachable'), 'should include error message');
  });
});

describe('normalize utility', () => {
  const { normalize, parseSize, parseDate, buildMagnet, extractInfoHash } = require('../src/lib/normalize');

  it('parses size strings correctly', () => {
    equal(parseSize('1.2 GB'), Math.round(1.2 * 1024 ** 3));
    equal(parseSize('800 MiB'), Math.round(800 * 1024 ** 2));
    equal(parseSize('512 MB'), 512 * 1024 ** 2);
    equal(parseSize('2048'), 2048); // bytes
    equal(parseSize(null), null);
    equal(parseSize(undefined), null);
  });

  it('parses date strings correctly', () => {
    // Unix timestamp (seconds)
    const ts = Math.floor(Date.now() / 1000);
    equal(parseDate(ts), ts * 1000);

    // Unix timestamp (milliseconds)
    const ms = Date.now();
    equal(parseDate(ms), ms);

    // Relative dates
    const now = Date.now();
    ok(parseDate('2 hours ago') <= now);
    ok(parseDate('3 days ago') < parseDate('2 hours ago'));
    ok(parseDate('yesterday') < parseDate('today'));
  });

  it('builds magnet URI from infoHash', () => {
    const magnet = buildMagnet('ABC123', 'Test Torrent');
    match(magnet, /^magnet:\?xt=urn:btih:ABC123&dn=Test%20Torrent$/);

    const magnetNoName = buildMagnet('ABC123');
    match(magnetNoName, /^magnet:\?xt=urn:btih:ABC123$/);

    equal(buildMagnet(null), null);
  });

  it('extracts infoHash from magnet URI', () => {
    const hash = extractInfoHash('magnet:?xt=urn:btih:ABC123DEF456&dn=test');
    equal(hash, 'abc123def456'); // normalized to lowercase

    equal(extractInfoHash('not a magnet'), null);
    equal(extractInfoHash(null), null);
  });

  it('normalizes raw result to canonical shape', () => {
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

    equal(normalized.id, 'tpb:ABC123');
    equal(normalized.name, 'Test Torrent');
    equal(normalized.provider, 'tpb');
    equal(normalized.size, Math.round(1.5 * 1024 ** 3));
    equal(normalized.sizeText, '1.5 GB');
    equal(normalized.seeders, 100);
    equal(normalized.leechers, 10);
    equal(normalized.date, 1609459200000);
    equal(normalized.category, 'Apps');
    equal(normalized.detailUrl, 'https://example.com/torrent/123');
    equal(normalized.infoHash, 'ABC123');
    ok(normalized.magnet?.startsWith('magnet:?'));
    equal(normalized.needsMagnet, false); // has infoHash, so magnet is built
  });

  it('marks result as needing magnet when no infoHash', () => {
    const raw = {
      provider: '1337x',
      name: 'Test Torrent',
      size: '500 MB',
      detailUrl: 'https://1337x.to/torrent/123/',
    };

    const normalized = normalize(raw);
    equal(normalized.needsMagnet, true);
    equal(normalized.magnet, null);
  });
});
