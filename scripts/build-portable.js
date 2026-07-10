'use strict';

// 手动拼装「免安装便携版」——纯文件拷贝，不依赖 electron-builder，
// 因此不会触发代码签名（winCodeSign/signtool）导致的卡死。
// 产物：dist/portable/BT聚合搜索/（双击 BT聚合搜索.exe 即用）。
//
// 顺序：
//   1. 拷贝 node_modules/electron/dist 作为运行时（含 electron.exe）
//   2. 删除默认的 default_app.asar，放入我们的代码到 resources/app/
//   3. 生产依赖（express/axios/cheerio）由 CI/本地后续 `npm install --omit=dev` 安装
//   4. 把 electron.exe 改名为 BT聚合搜索.exe

const fs = require('fs');
const path = require('path');

const ROOT = path.resolve(__dirname, '..');
const ELECTRON_DIST = path.join(ROOT, 'node_modules', 'electron', 'dist');
const PORTABLE = path.join(ROOT, 'dist', 'portable', 'BT聚合搜索');
const APP = path.join(PORTABLE, 'resources', 'app');

function rmrf(p) {
  if (fs.existsSync(p)) fs.rmSync(p, { recursive: true, force: true });
}

function copyDir(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  for (const e of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, e.name);
    const d = path.join(dest, e.name);
    if (e.isDirectory()) copyDir(s, d);
    else fs.copyFileSync(s, d);
  }
}

function main() {
  if (!fs.existsSync(ELECTRON_DIST)) {
    throw new Error('未找到 electron 运行时，请先执行 `npm ci`（会下载 electron 二进制）');
  }

  rmrf(PORTABLE);
  fs.mkdirSync(APP, { recursive: true });

  // 1. electron 运行时
  copyDir(ELECTRON_DIST, PORTABLE);
  const defaultApp = path.join(PORTABLE, 'resources', 'default_app.asar');
  if (fs.existsSync(defaultApp)) fs.rmSync(defaultApp);

  // 2. 应用源码
  for (const f of ['server.js', 'package.json']) {
    fs.copyFileSync(path.join(ROOT, f), path.join(APP, f));
  }
  for (const d of ['electron', 'public', 'src']) {
    copyDir(path.join(ROOT, d), path.join(APP, d));
  }

  // 3. 改名 exe
  const exeFrom = path.join(PORTABLE, 'electron.exe');
  const exeTo = path.join(PORTABLE, 'BT聚合搜索.exe');
  if (fs.existsSync(exeFrom)) fs.renameSync(exeFrom, exeTo);

  console.log('[build-portable] 已生成：', PORTABLE);
}

main();
