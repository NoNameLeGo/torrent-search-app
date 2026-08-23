// Minify frontend JS and CSS for production into public/dist/.
// ES modules are kept as separate files (dynamic imports break circular deps).
// Only individual minification — no bundling.
const esbuild = require('esbuild');
const fs = require('fs');
const path = require('path');

const publicDir = path.join(__dirname, '..', 'public');
const distDir = path.join(publicDir, 'dist');

async function build() {
  fs.mkdirSync(path.join(distDir, 'js'), { recursive: true });

  const jsDir = path.join(publicDir, 'js');
  const files = fs.readdirSync(jsDir).filter((f) => f.endsWith('.js') && !f.endsWith('.min.js'));

  for (const file of files) {
    const src = path.join(jsDir, file);
    const dst = path.join(distDir, 'js', file);
    await esbuild.build({
      entryPoints: [src],
      outfile: dst,
      minify: true,
      format: 'esm',
      target: ['chrome100'],
    });
  }

  // Minify ambient.js (non-module)
  await esbuild.build({
    entryPoints: [path.join(publicDir, 'ambient.js')],
    outfile: path.join(distDir, 'ambient.js'),
    minify: true,
    format: 'iife',
    target: ['chrome100'],
  });

  // Minify CSS
  await esbuild.build({
    entryPoints: [path.join(publicDir, 'styles.css')],
    outfile: path.join(distDir, 'styles.css'),
    minify: true,
    loader: { '.css': 'css' },
  });

  // Copy HTML
  fs.copyFileSync(path.join(publicDir, 'index.html'), path.join(distDir, 'index.html'));

  console.log('Frontend minified → public/dist/');
}

build().catch((e) => { console.error(e); process.exit(1); });