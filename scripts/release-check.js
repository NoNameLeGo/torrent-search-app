'use strict';

/**
 * 打 tag 发布前的预检脚本（npm run release:check）
 *
 * 2026-08-14 事故复盘：本地仓库历史被重置后，feat/tauri 分支与
 * release.yml/tauri-build.yml 一并从本地消失，打 tag 不会触发
 * Electron+Tauri 双构建 Release，而流程「看起来丢了」。
 * 此脚本在打 tag 前兜底检查分支、workflow、工作区与版本状态。
 *
 * Windows 兼容：全部用 execFileSync 直连 git/node，不走 shell，
 * 避免 cmd 不识别 `2>/dev/null`、`&&` 等 Unix 语法。
 */
const { execFileSync } = require('child_process');

function gitOut(args) {
  try {
    return execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
  } catch (e) {
    return '';
  }
}

function gitFileExists(rev, path) {
  try {
    execFileSync('git', ['cat-file', '-e', `${rev}:${path}`], { stdio: 'ignore' });
    return true;
  } catch (e) {
    return false;
  }
}

let failed = false;
function check(ok, msg) {
  console.log(`${ok ? '✓' : '✗'} ${msg}`);
  if (!ok) failed = true;
}

console.log('== release:check 预检 ==');

// 1) 当前分支
const branch = gitOut(['rev-parse', '--abbrev-ref', 'HEAD']);
check(!!branch, `当前分支：${branch || '(无)'}`);

// 2) feat/tauri 分支存在（release 双构建依赖，建议从 feat/tauri 打 tag）
const hasFeat = !!gitOut(['rev-parse', '--verify', '--quiet', 'feat/tauri']);
check(hasFeat, 'feat/tauri 分支存在');

// 3) release.yml 存在（main 或 feat/tauri 上均可）
const relMain = gitFileExists('main', '.github/workflows/release.yml');
const relFeat = hasFeat && gitFileExists('feat/tauri', '.github/workflows/release.yml');
check(relMain || relFeat, `release.yml 存在（main=${relMain ? '✓' : '✗'} feat/tauri=${relFeat ? '✓' : '✗'}）`);

// 4) tauri-build.yml 存在（Tauri 双构建另一半）
const tbMain = gitFileExists('main', '.github/workflows/tauri-build.yml');
const tbFeat = hasFeat && gitFileExists('feat/tauri', '.github/workflows/tauri-build.yml');
check(tbMain || tbFeat, `tauri-build.yml 存在（main=${tbMain ? '✓' : '✗'} feat/tauri=${tbFeat ? '✓' : '✗'}）`);

// 5) 工作区干净（打 tag 前不应有未提交改动）
const dirty = gitOut(['status', '--porcelain']);
check(dirty === '', dirty === '' ? '工作区干净' : `工作区有未提交改动（${dirty.split('\n').length} 项）`);

// 6) 版本号与最近 tag
let version = '';
try {
  version = execFileSync(process.execPath, ['-p', "require('./package.json').version"], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim();
} catch (e) { /* ignore */ }
const lastTag = gitOut(['describe', '--tags', '--abbrev=0']);
console.log(`   版本：package.json=${version || '(空)'}   最近 tag：${lastTag || '(无)'}`);

// 7) 未推送的提交（相对 origin/main；无 origin 时不阻塞）
const unpushed = gitOut(['log', '--oneline', 'origin/main..HEAD']);
if (unpushed) {
  console.log('   ⚠️ 有未推送到 origin/main 的提交：');
  unpushed.split('\n').slice(0, 5).forEach((l) => console.log('     ' + l));
}

console.log(failed ? '\n✗ 预检未通过，请先修复以上问题' : '\n✓ 预检通过，可以打 tag');
process.exit(failed ? 1 : 0);
