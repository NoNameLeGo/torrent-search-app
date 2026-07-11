// Prepares the Tauri sidecar binary.
//
// Route A bundles the REAL Node.js runtime as the sidecar (not a pkg-compiled
// exe). We simply copy the node executable that is running this script into
// src-tauri/binaries/server-<target-triple>.exe, which is what Tauri expects
// for `bundle.externalBin`. The backend code (server.js + src/ + node_modules)
// and the frontend (public/) are shipped separately as Tauri `bundle.resources`,
// and Rust launches the sidecar with `server.js` as the first argument.
//
// Output: src-tauri/binaries/server-<target-triple>.exe  (git-ignored)
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execSync } from 'node:child_process';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, '..');
const bins = path.join(root, 'src-tauri', 'binaries');
fs.mkdirSync(bins, { recursive: true });

const nodeExe = process.execPath;

const triple =
  process.env.TARGET_TRIPLE ||
  (() => {
    try {
      return execSync('rustc --print host-tuple').toString().trim();
    } catch {
      return 'x86_64-pc-windows-msvc';
    }
  })();

const ext = process.platform === 'win32' ? '.exe' : '';
const out = path.join(bins, `server-${triple}${ext}`);

fs.copyFileSync(nodeExe, out);
console.log(`sidecar (node.exe) prepared at: ${out}`);
