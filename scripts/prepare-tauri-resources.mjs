// 构建前：把 node_modules 收敛为「仅生产依赖」，作为 Tauri bundle.resources 的 payload。
//
// 背景：`npm prune --omit=dev` 实测会残留 dev 依赖的孤儿子图
// （例如 @tauri-apps/cli 的平台二进制 @tauri-apps/cli-win32-x64-msvc、@electron/* 等），
// 导致 v0.3.4 之前的安装包白白多带 15MB 以上；对已存在的 node_modules 再跑
// prune / `npm install --omit=dev` 也无法清除这些残留（npm 对已落盘的孤儿不动）。
// 唯一确定性方案：删除 node_modules 后做一次全新生产安装——只会装 package.json
// `dependencies` 的完整闭包（axios / cheerio / express 及其传递依赖，实测约 98 个顶层包）。
//
// 副作用：执行后本目录 node_modules 只剩运行时依赖；要继续做开发请重新 `npm install`。
// 该脚本在 tauri.conf.json 的 beforeBuildCommand 中、build:sidecar 之前执行，
// 需要联网（或 npm 缓存命中）。之后 verify-tauri-resources.mjs 会再兜底校验一次。
import fs from 'node:fs';
import path from 'node:path';
import { execSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, '..');

const nm = path.join(root, 'node_modules');

// 逐项删除 node_modules 顶层条目，跳过正在被 tauri build 进程锁定的文件。
// 直接用 fs.rmSync(nm, { recursive: true }) 在 Windows 上会因 @tauri-apps/cli
// 的原生二进制被占用而整个失败（EPERM）。逐项删除 + 跳过 @tauri-apps 即可避让。
function rmNodeModules() {
  if (!fs.existsSync(nm)) return;
  const entries = fs.readdirSync(nm);
  // @tauri-apps 需要保留：tauri build 进程本身正在使用其原生二进制，
  // Windows 不允许删除已加载的 .node 文件。
  const skip = new Set(['@tauri-apps', '.package-lock.json']);
  let failed = 0;
  for (const name of entries) {
    if (skip.has(name)) {
      console.log(`[prepare-tauri-resources]   跳过 ${name}（被构建进程占用，保留）`);
      continue;
    }
    const full = path.join(nm, name);
    try {
      fs.rmSync(full, { recursive: true, force: true });
    } catch (e) {
      console.warn(`[prepare-tauri-resources]   无法删除 ${name}: ${e.code ?? e.message}`);
      failed++;
    }
  }
  if (failed > 0) {
    console.warn(
      `[prepare-tauri-resources] ⚠ ${failed} 个条目删除失败（可能被占用），继续安装…`,
    );
  }
}

console.log('[prepare-tauri-resources] 删除现有 node_modules（含 dev 依赖），准备全新生产安装…');
rmNodeModules();

console.log('[prepare-tauri-resources] npm install --omit=dev --ignore-scripts …');
execSync('npm install --omit=dev --ignore-scripts', { cwd: root, stdio: 'inherit' });
console.log('[prepare-tauri-resources] node_modules 已收敛为仅生产依赖。');
