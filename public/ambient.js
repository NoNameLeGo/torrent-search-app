/* ambient.js — 「青瓷」环境粒子
   低透明度、慢速上漂的微尘，颜色取自主题 token（墨/青/釉），仅作氛围。
   - 总数 ≤ 56，按视口面积自适应
   - prefers-reduced-motion: reduce 时完全不启动
   - 页面隐藏（visibilitychange）时暂停渲染
   - 搜索提交时一次克制的向心呼应（约 0.7s）
   画布 z-index:-1，位于所有文字层之下，不干扰阅读。 */
(() => {
  'use strict';
  const cv = document.getElementById('ambient');
  if (!cv) return;
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;

  const ctx = cv.getContext('2d');
  // 主题 token 的 RGB 分量：Mocha Text #cdd6f4 / Green #a6e3a1 / Teal #94e2d5（暗底亮尘）
  const PALETTE = ['205, 214, 244', '166, 227, 161', '148, 226, 213'];

  let W = 0, H = 0, parts = [], raf = null, gatherUntil = 0;

  const spawn = () => {
    const roll = Math.random();
    return {
      x: Math.random() * W,
      y: Math.random() * H,
      r: 0.8 + Math.random() * 1.6,
      a: 0.05 + Math.random() * 0.09,
      vx: (Math.random() - 0.5) * 0.22,
      vy: -(0.05 + Math.random() * 0.16),
      ph: Math.random() * Math.PI * 2,
      c: roll < 0.72 ? PALETTE[0] : roll < 0.86 ? PALETTE[1] : PALETTE[2],
    };
  };

  const resize = () => {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    W = window.innerWidth;
    H = window.innerHeight;
    cv.width = Math.round(W * dpr);
    cv.height = Math.round(H * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    const n = Math.min(56, Math.max(18, Math.round((W * H) / 32000)));
    parts = Array.from({ length: n }, spawn);
  };

  const tick = (t) => {
    raf = requestAnimationFrame(tick);
    ctx.clearRect(0, 0, W, H);
    const gather = t < gatherUntil;
    for (const p of parts) {
      p.x += p.vx + Math.sin(t / 4000 + p.ph) * 0.05;
      p.y += p.vy;
      if (gather) { // 搜索时的轻微向心，随后自然散开
        p.x += (W / 2 - p.x) * 0.0035;
        p.y += (H / 2 - p.y) * 0.0035;
      }
      if (p.y < -6) { p.y = H + 6; p.x = Math.random() * W; }
      if (p.x < -6) p.x = W + 6;
      else if (p.x > W + 6) p.x = -6;
      ctx.globalAlpha = p.a;
      ctx.fillStyle = `rgb(${p.c})`;
      ctx.beginPath();
      ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  };

  const start = () => { if (!raf) raf = requestAnimationFrame(tick); };
  const stop = () => { if (raf) { cancelAnimationFrame(raf); raf = null; } };

  document.addEventListener('visibilitychange', () => (document.hidden ? stop() : start()));
  window.addEventListener('resize', resize);

  const form = document.getElementById('search-form');
  if (form) form.addEventListener('submit', () => { gatherUntil = performance.now() + 700; });

  resize();
  start();
})();
