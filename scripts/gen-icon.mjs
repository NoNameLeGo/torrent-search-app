// Regenerates the Tauri app icons (src-tauri/icons/*) from a simple generated
// PNG. Run with: node scripts/gen-icon.mjs
import zlib from 'node:zlib';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, '../src-tauri/icons');
fs.mkdirSync(outDir, { recursive: true });

function makePixels(size) {
  const px = Buffer.alloc(size * size * 4);
  const cx = size / 2;
  const cy = size / 2;
  const r = size * 0.38;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const i = (y * size + x) * 4;
      const dx = x - cx;
      const dy = y - cy;
      const dist = Math.sqrt(dx * dx + dy * dy);
      let R, G, B;
      if (dist <= r) {
        const t = dist / r;
        R = Math.round(99 + (1 - t) * 80);
        G = Math.round(102 + (1 - t) * 60);
        B = Math.round(241 + (1 - t) * 14);
      } else {
        R = 15;
        G = 17;
        B = 21;
      }
      px[i] = R;
      px[i + 1] = G;
      px[i + 2] = B;
      let a = 255;
      if (dist > r + 2) a = Math.max(0, 255 - Math.round((dist - (r + 2)) * 30));
      px[i + 3] = a;
    }
  }
  return px;
}

function crc32(buf) {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, 'ascii');
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])) >>> 0, 0);
  return Buffer.concat([len, typeBuf, data, crc]);
}

function makePNG(size, px) {
  const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // color type RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    px.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, y * size * 4 + size * 4);
  }
  const idat = zlib.deflateSync(raw);
  return Buffer.concat([sig, chunk('IHDR', ihdr), chunk('IDAT', idat), chunk('IEND', Buffer.alloc(0))]);
}

function makeICO(png256) {
  const icondir = Buffer.alloc(6);
  icondir.writeUInt16LE(0, 0);
  icondir.writeUInt16LE(1, 2);
  icondir.writeUInt16LE(1, 4);
  const entry = Buffer.alloc(16);
  entry[0] = 0; // width 0 => 256
  entry[1] = 0; // height 0 => 256
  entry[2] = 0;
  entry[3] = 0;
  entry.writeUInt16LE(1, 4); // color planes
  entry.writeUInt16LE(32, 6); // bpp
  entry.writeUInt32LE(png256.length, 8); // image size
  entry.writeUInt32LE(6 + 16, 12); // image offset
  return Buffer.concat([icondir, entry, png256]);
}

const png256 = makePNG(256, makePixels(256));
fs.writeFileSync(path.join(outDir, 'icon.png'), png256);
fs.writeFileSync(path.join(outDir, '32x32.png'), makePNG(32, makePixels(32)));
fs.writeFileSync(path.join(outDir, '128x128.png'), makePNG(128, makePixels(128)));
fs.writeFileSync(path.join(outDir, '128x128@2x.png'), png256);
fs.writeFileSync(path.join(outDir, 'icon.ico'), makeICO(png256));
console.log('icons written to', outDir);
