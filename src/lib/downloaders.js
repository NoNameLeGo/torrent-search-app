'use strict';

// 下载客户端统一封装：把磁力推送到本机运行的下载器 WebUI / RPC。
// 目前支持 qBittorrent、Transmission、aria2（含 Motrix，底层就是 aria2）、Gopeed。
//
// 约定（与 provider 层「永不抛异常」相反，这里刻意用抛异常表达失败）：
//   - push(cfg, magnet) / test(cfg) 成功时 resolve，失败时 throw Error(可读文案)；
//   - server 层负责 try/catch 转成 { ok:false, error } 响应。
// 这样每种客户端的错误细节（登录失败 / RPC 报错 / 连不上）都能透传给前端提示。
//
// 为什么不放进 src/lib/http.js：那层是给「抓取第三方页面」用的（UA 轮换、never-throw、
// 返回 {html|data,error}）；这里是给「本机受信下载器」用的，需要 Basic Auth、会话 cookie、
// JSON-RPC、409 重试等各不相同的协议细节，语义上是另一件事，独立成模块更清晰。

const axios = require('axios');

// 每个下载器都在本机（localhost / 127.0.0.1）跑，超时给短一点，连不上尽快失败。
const TIMEOUT = 10000;

// 把用户填的基址补上该客户端的固定路径；若用户已经把完整路径填进去了就原样用。
// 例：base=http://localhost:9091 + /transmission/rpc → http://localhost:9091/transmission/rpc
function withPath(base, path) {
  const b = String(base || '').replace(/\/+$/, '');
  if (b.toLowerCase().endsWith(path.toLowerCase())) return b;
  return b + path;
}

// ---------------------------------------------------------------------------
// qBittorrent WebUI (api/v2)
// 先 /auth/login 拿 session cookie，再带 cookie 调 /torrents/add。
// ---------------------------------------------------------------------------
async function qbLogin(base, user, pass) {
  const login = await axios.post(
    `${base}/api/v2/auth/login`,
    `username=${encodeURIComponent(user || '')}&password=${encodeURIComponent(pass || '')}`,
    { headers: { 'Content-Type': 'application/x-www-form-urlencoded' }, timeout: 6000 }
  );
  const cookie = login.headers['set-cookie'] && login.headers['set-cookie'][0];
  if (!cookie || /fails|failed/i.test(String(login.data))) throw new Error('登录失败（用户名或密码错误）');
  return cookie;
}

const qbittorrent = {
  async push(cfg, magnet) {
    const base = String(cfg.url).replace(/\/+$/, '');
    const cookie = await qbLogin(base, cfg.user, cfg.pass);
    const add = await axios.post(
      `${base}/api/v2/torrents/add`,
      `urls=${encodeURIComponent(magnet)}`,
      { headers: { 'Content-Type': 'application/x-www-form-urlencoded', Cookie: cookie }, timeout: TIMEOUT }
    );
    return { status: add.status };
  },
  // 疎通確認：仅登录，不产生任何下载副作用。
  async test(cfg) {
    const base = String(cfg.url).replace(/\/+$/, '');
    await qbLogin(base, cfg.user, cfg.pass);
    return { ok: true };
  },
};

// ---------------------------------------------------------------------------
// Transmission RPC
// POST /transmission/rpc；首次调用常返回 409 并在响应头带 X-Transmission-Session-Id，
// 这是它的 CSRF 防护，取到该 id 后带上重发即可。可选 Basic Auth。
// ---------------------------------------------------------------------------
async function transmissionCall(cfg, body) {
  const url = withPath(cfg.url, '/transmission/rpc');
  const auth = (cfg.user || cfg.pass) ? { username: cfg.user || '', password: cfg.pass || '' } : undefined;
  const headers = { 'Content-Type': 'application/json' };
  try {
    const r = await axios.post(url, body, { headers, auth, timeout: TIMEOUT });
    return r.data;
  } catch (e) {
    const sid = e.response && e.response.status === 409 && e.response.headers['x-transmission-session-id'];
    if (!sid) throw e;
    const r = await axios.post(url, body, {
      headers: { ...headers, 'X-Transmission-Session-Id': sid }, auth, timeout: TIMEOUT,
    });
    return r.data;
  }
}

const transmission = {
  async push(cfg, magnet) {
    const data = await transmissionCall(cfg, { method: 'torrent-add', arguments: { filename: magnet } });
    if (data && data.result && data.result !== 'success') throw new Error('Transmission: ' + data.result);
    return { status: 200 };
  },
  async test(cfg) {
    const data = await transmissionCall(cfg, { method: 'session-get' });
    if (data && data.result && data.result !== 'success') throw new Error('Transmission: ' + data.result);
    return { ok: true };
  },
};

// ---------------------------------------------------------------------------
// aria2 / Motrix JSON-RPC
// Motrix 底层就是 aria2，接口一致，仅默认端口不同（Motrix 16800 / aria2 6800）。
// POST 到 /jsonrpc；若设了 rpc-secret，需作为首个参数 'token:<secret>' 传入。
// ---------------------------------------------------------------------------
async function aria2Call(cfg, method, params) {
  const url = withPath(cfg.url, '/jsonrpc');
  const secret = cfg.token ? [`token:${cfg.token}`] : [];
  const body = { jsonrpc: '2.0', id: 'ts', method, params: [...secret, ...(params || [])] };
  const r = await axios.post(url, body, { headers: { 'Content-Type': 'application/json' }, timeout: TIMEOUT });
  if (r.data && r.data.error) {
    throw new Error('aria2: ' + (r.data.error.message || JSON.stringify(r.data.error)));
  }
  return r.data && r.data.result;
}

const aria2 = {
  async push(cfg, magnet) {
    await aria2Call(cfg, 'aria2.addUri', [[magnet]]);
    return { status: 200 };
  },
  async test(cfg) {
    await aria2Call(cfg, 'aria2.getVersion', []);
    return { ok: true };
  },
};

// ---------------------------------------------------------------------------
// Gopeed REST（默认端口 9999）
// POST /api/v1/tasks，body { req: { url: <magnet> } }。若设了 API Token，用请求头带上；
// 若开了 Basic Auth（config 里 username/password），走 auth。二者按用户所填择一。
// ---------------------------------------------------------------------------
const gopeed = {
  async push(cfg, magnet) {
    const url = withPath(cfg.url, '/api/v1/tasks');
    const headers = { 'Content-Type': 'application/json' };
    if (cfg.token) headers['X-Api-Token'] = cfg.token;
    const auth = (cfg.user || cfg.pass) ? { username: cfg.user || '', password: cfg.pass || '' } : undefined;
    const r = await axios.post(url, { req: { url: magnet } }, { headers, auth, timeout: TIMEOUT });
    // Gopeed 返回 { code, msg, data }，code!==0 视为失败。
    if (r.data && typeof r.data.code === 'number' && r.data.code !== 0) {
      throw new Error('Gopeed: ' + (r.data.msg || ('code ' + r.data.code)));
    }
    return { status: r.status };
  },
  async test(cfg) {
    // 列任务作轻量疎通：能拿到 2xx 即认为可达 + 认证通过，不产生下载副作用。
    const url = withPath(cfg.url, '/api/v1/tasks');
    const headers = {};
    if (cfg.token) headers['X-Api-Token'] = cfg.token;
    const auth = (cfg.user || cfg.pass) ? { username: cfg.user || '', password: cfg.pass || '' } : undefined;
    await axios.get(url, { headers, auth, timeout: TIMEOUT });
    return { ok: true };
  },
};

const CLIENTS = { qbittorrent, transmission, aria2, gopeed };

// 客户端元信息：前端下拉展示 + 探测用的默认候选。
// label 给 UI 显示；auth 决定设置面板显示哪些字段（用户名密码 / token）。
const META = {
  qbittorrent: { label: 'qBittorrent', auth: 'userpass', defaultUrl: 'http://localhost:8080' },
  transmission: { label: 'Transmission', auth: 'userpass', defaultUrl: 'http://localhost:9091' },
  aria2: { label: 'aria2 / Motrix', auth: 'token', defaultUrl: 'http://localhost:16800/jsonrpc' },
  gopeed: { label: 'Gopeed', auth: 'token', defaultUrl: 'http://localhost:9999' },
};

// 本机自动探测候选：按客户端默认端口 + 无/默认凭据逐个尝试疎通。
// 只覆盖「开箱默认」情形；设了非默认端口 / 密码的用户仍可手动填写。
const DETECT_CANDIDATES = [
  { kind: 'qbittorrent', url: 'http://localhost:8080', user: 'admin', pass: 'adminadmin' },
  { kind: 'qbittorrent', url: 'http://localhost:8080', user: 'admin', pass: '' },
  { kind: 'qbittorrent', url: 'http://127.0.0.1:8080', user: 'admin', pass: 'adminadmin' },
  { kind: 'qbittorrent', url: 'http://localhost:8081', user: 'admin', pass: 'adminadmin' },
  { kind: 'transmission', url: 'http://localhost:9091' },
  { kind: 'transmission', url: 'http://127.0.0.1:9091' },
  { kind: 'aria2', url: 'http://localhost:16800/jsonrpc' }, // Motrix 默认
  { kind: 'aria2', url: 'http://localhost:6800/jsonrpc' },  // aria2 默认
  { kind: 'gopeed', url: 'http://localhost:9999' },
];

// 推送磁力。kind 未知直接抛错。
async function push(kind, cfg, magnet) {
  const client = CLIENTS[kind];
  if (!client) throw new Error('unknown_client');
  return client.push(cfg, magnet);
}

// 测试连接。
async function test(kind, cfg) {
  const client = CLIENTS[kind];
  if (!client) throw new Error('unknown_client');
  return client.test(cfg);
}

// 逐个候选尝试疎通，命中第一个即返回其配置；全不中返回 null。
async function detect() {
  for (const c of DETECT_CANDIDATES) {
    try {
      await test(c.kind, c);
      return c;
    } catch (e) { /* 试下一个 */ }
  }
  return null;
}

module.exports = { push, test, detect, META, CLIENTS };
