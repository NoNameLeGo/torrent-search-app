# torrent-search-app — 项目长期记忆

## 网盘 / PanSou 决策（2026-10-25）

- **先做方案 A**：App 只当 PanSou 的客户端，端点在设置里配（默认 `http://127.0.0.1:8888`），不内置、不管理 sidecar。
- **方案 B**（首次下载 `pansou.exe` + Windows Job Object 随开随停）以后再说。
- **不要把 Go 二进制塞进主 exe**：主 exe 约 19.5MB、门限 25MB，且“单文件自包含”是卖点（D7 / S2 / M7）。
- PanSou API（上游 `fish2018/pansou`，MIT）：
  - `POST /api/search`，body `{"kw":"...","res":"merge"}`；可选 `Authorization: Bearer <token>`。
  - 响应 `merged_by_type: { quark: [{url,password,note,datetime,source}], baidu: [...], ... }`。
  - `GET /api/health` 探活；`PORT` 环境变量改端口（默认 8888）。
  - 公开实例 `pansou.de` 不暴露 `/api/search`（404），只有「自建」或「逆向 Next.js server action」两条路。
- 实现口径：Pan 结果与 BT / Video **分开**（第三种结果类型），不混流；点击打开网盘链接 + 复制提取码。
