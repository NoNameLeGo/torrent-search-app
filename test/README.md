# Test Runner Guide — Agent Call Instructions

## Quick Commands

```bash
# Run all tests
npm test

# Run specific test file
node test/run.js              # Provider tests (TPB, LinuxTracker)
node test/normalize.test.js   # Normalize utility tests

# Watch mode (requires nodemon)
npm run test:watch
```

## When to Run Tests

| Scenario | Action |
|----------|--------|
| 修改 provider 解析逻辑 | `npm test` 验证结果形状 |
| 修改 normalize.js | `npm test` + `node test/normalize.test.js` |
| 添加新 provider | 先写 fixture，再写测试，最后 `npm test` |
| PR 提交前 | 确保所有测试通过 |

## Test Structure

```
test/
  run.js                  ← 主入口，测试 TPB + Knaben + LinuxTracker + FileMood
  normalize.test.js       ← normalize.js 单元测试
  fixtures/               ← 真实 HTML/JSON 快照
    tpb-ubuntu.json       ← TPB API 响应 (100 results)
    knaben-ubuntu.json    ← Knaben API 响应 (5 results)
    linuxtracker-linux.html ← LinuxTracker HTML (18 results)
    filemood-ubuntu.html  ← FileMood HTML (20 results)
```

## How to Add a New Provider Test

### Step 1: Save Fixture
```bash
# 手动抓取真实响应，保存到 test/fixtures/<provider>-<query>.html 或 .json
node -e "
const https = require('https');
const fs = require('fs');
const url = 'https://example.com/search?q=test';
const out = 'test/fixtures/example-search.html';
https.get(url, { headers: { 'User-Agent': 'Mozilla/5.0' } }, (res) => {
  let data = '';
  res.on('data', c => data += c);
  res.on('end', () => fs.writeFileSync(out, data));
});
"
```

### Step 2: Add Test Block
在 `test/run.js` 末尾添加：
```javascript
console.log('=== ExampleProvider ===\n');

(async () => {
  console.log('Test 1: Parse HTML fixture...');
  const html = loadHTML('example-search.html');
  const cleanup = createMockHTTPForHTML([
    [/(?:^https?:)?\/\/example\.com/i, { html, error: null }]
  ]);

  const { search } = require('../src/providers/example');
  const result = await search('test', { page: 1 });
  cleanup();

  assert.ok(!result.error, 'should not have error');
  assert.ok(Array.isArray(result.results), 'results should be array');
  assert.ok(result.results.length > 0, 'should have results');

  const first = result.results[0];
  assert.equal(first.provider, 'example', 'provider should match');
  assert.ok(first.name, 'should have name');
  assert.ok(typeof first.size === 'number', 'size should be parsed');

  console.log(`  ✓ Found ${result.results.length} results\n`);
})();
```

### Step 3: Run Tests
```bash
npm test
```

## Mock HTTP Functions

| Function | Use Case |
|----------|----------|
| `createMockHTTP(responses)` | JSON API providers (tpb, apibay) |
| `createMockHTTPForHTML(responses)` | HTML scraper providers (linuxtracker, rutor) |

### Response Format
```javascript
[
  [/regex-or-prefix/, { data: fixtureData, error: null }],
  [/another-domain/i, { html: fixtureHTML, error: null }]
]
```

## Current Coverage

| Provider | Tests | Status |
|----------|-------|--------|
| `tpb.js` | search parsing, category mapping, empty results, HTTP error | ✅ |
| `knaben.js` | JSON API (POST) parsing, category mapping | ✅ |
| `linuxtracker.js` | HTML parsing, infoHash extraction, size/date/seeds | ✅ |
| `filemood.js` | HTML parsing, infoHash extraction from URL, size/seeds | ✅ |
| `normalize.js` | parseSize, parseDate, buildMagnet, extractInfoHash, ruDate | ✅ |

## Troubleshooting

### Test fails with "EPERM"
- Windows 权限问题，尝试以管理员身份运行
- 或检查是否有其他进程占用文件

### Mock not working
- 确保调用 `cleanup()` 清理 mock
- 检查 RegExp 是否正确匹配 URL

### Fixture missing
- 确认文件在 `test/fixtures/` 目录
- 使用 `loadJSON()` 或 `loadHTML()` 加载
