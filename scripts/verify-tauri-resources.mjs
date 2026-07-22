// 构建前资源校验：确保 Tauri 打包所需的关键资源就位，
// 否则构建会“成功”却产出打不开的安装包（sidecar 起不来 → 127.0.0.1 错误）。
// 该脚本在 tauri.conf.json 的 beforeBuildCommand 末尾执行，
// 失败则非零退出，阻断打包，把问题暴露在 CI / 本地构建阶段。
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const checks = [
  { path: 'server.js', label: 'server.js（后端入口）' },
  { path: 'public/index.html', label: 'public/index.html（前端静态页）' },
  { path: 'src', label: 'src/（搜索 provider 代码）' },
  { path: 'node_modules/express', label: 'node_modules/express（后端运行时依赖）' },
];

const missing = checks.filter((c) => !fs.existsSync(path.join(root, c.path)));

// sidecar 二进制：prepare-sidecar 产出 server-<triple>.exe
let sidecarOk = false;
const binsDir = path.join(root, 'src-tauri', 'binaries');
if (fs.existsSync(binsDir)) {
  sidecarOk = fs.readdirSync(binsDir).some((f) => f.startsWith('server-'));
}

const errors = [];
for (const c of missing) errors.push(`  x 缺失 ${c.label}（${c.path}）`);
if (!sidecarOk) {
  errors.push('  x 缺失 sidecar 二进制（src-tauri/binaries/server-<triple>.exe），请先运行 npm run build:sidecar');
}

if (errors.length > 0) {
  console.error(
    '\n[资源校验失败] 以下资源未就位，打出的安装包将无法运行：\n' +
      errors.join('\n') +
      '\n\n请确认已执行 npm install，且 beforeBuildCommand 已正确运行 build:sidecar。\n',
  );
  process.exit(1);
}

console.log('[资源校验通过] server.js / public / src / node_modules / sidecar 二进制均就位。');
