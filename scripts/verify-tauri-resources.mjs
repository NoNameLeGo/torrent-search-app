// 构建前资源校验：确保 Tauri 打包所需的关键资源就位，且 node_modules 不含 dev 依赖。
// 该脚本在 tauri.conf.json 的 beforeBuildCommand 末尾执行，
// 失败则非零退出，阻断打包，把问题暴露在 CI / 本地构建阶段。
//
// 覆盖两类历史事故：
// 1) 打包漏掉 / 缺资源（sidecar 起不来 → 127.0.0.1 错误）
// 2) `npm prune --omit=dev` 残留 dev 孤儿子图（安装包多带 15MB @tauri-apps/cli 等）
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const errors = [];

// ---------- 1) 关键资源存在性 ----------
const checks = [
  { path: 'server.js', label: 'server.js（后端入口）' },
  { path: 'public/index.html', label: 'public/index.html（前端静态页）' },
  { path: 'src', label: 'src/（搜索 provider 代码）' },
  { path: 'node_modules/express', label: 'node_modules/express（后端运行时依赖）' },
];
for (const c of checks) {
  if (!fs.existsSync(path.join(root, c.path))) {
    errors.push(`  x 缺失 ${c.label}（${c.path}）`);
  }
}

// ---------- 2) sidecar 二进制：prepare-sidecar 产出 server-<triple>.exe ----------
const binsDir = path.join(root, 'src-tauri', 'binaries');
const sidecarOk =
  fs.existsSync(binsDir) && fs.readdirSync(binsDir).some((f) => f.startsWith('server-'));
if (!sidecarOk) {
  errors.push(
    '  x 缺失 sidecar 二进制（src-tauri/binaries/server-<triple>.exe），请先运行 npm run build:sidecar',
  );
}

// ---------- 3) node_modules 不得残留 dev 依赖 ----------
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const devNames = Object.keys(pkg.devDependencies || {});
// 兜底名单：即使未来某依赖被误移出 devDependencies，也绝不允许被打进安装包。
// 注：@tauri-apps / @electron 不在本名单中——它们是构建工具链自身，
// tauri build 运行时其文件被锁定，prepare-tauri-resources.mjs 可能无法完全删除，属预期残留。
const extraDeny = [
  'electron',
  'electron-builder',
  'app-builder-lib',
  'builder-util',
  'builder-util-runtime',
  'dmg-builder',
  'electron-publish',
  'electron-winstaller',
  'electron-builder-squirrel-windows',
];

const nmRoot = path.join(root, 'node_modules');
const leftovers = new Set();
const existsName = (name) => fs.existsSync(path.join(nmRoot, ...name.split('/')));
for (const n of devNames) if (existsName(n)) leftovers.add(n);
for (const n of extraDeny) if (existsName(n)) leftovers.add(n);
// dev 作用域整体检查：@tauri-apps 下残留的孤儿（如 cli-win32-x64-msvc）不在 devNames 里。
// 但 @tauri-apps 是构建工具自身，tauri build 运行时其二进制被锁定无法删除，跳过。
const devScopes = new Set(
  devNames.filter((n) => n.startsWith('@') && !n.startsWith('@tauri-apps')).map((n) => n.split('/')[0]),
);
for (const scope of devScopes) {
  const dir = path.join(nmRoot, scope);
  if (fs.existsSync(dir) && fs.readdirSync(dir).length > 0) {
    for (const kid of fs.readdirSync(dir)) leftovers.add(`${scope}/${kid}`);
  }
}

if (leftovers.size > 0) {
  errors.push(
    '  x node_modules 仍残留 dev 依赖（会让安装包多带数十 MB）：\n' +
      [...leftovers].map((n) => `      · node_modules/${n}`).join('\n') +
      '\n    请确认 beforeBuildCommand 的 prepare-tauri-resources.mjs 已执行成功（需联网做全新生产安装）。',
  );
}

// ---------- 汇总 ----------
if (errors.length > 0) {
  console.error(
    '\n[资源校验失败] 以下问题会让打出的安装包无法运行或异常臃肿：\n' +
      errors.join('\n') +
      '\n\n请先修复后再构建。\n',
  );
  process.exit(1);
}

console.log(
  '[资源校验通过] server.js / public / src / node_modules(仅生产依赖) / sidecar 均就位。',
);
