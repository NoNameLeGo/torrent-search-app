'use strict';

const path = require('path');
const net = require('net');
const { app, BrowserWindow, dialog } = require('electron');
const { start } = require('../server');

let mainWindow = null;
let server = null;

// 取一个空闲端口，避免和已在跑的开发服务器冲突
function getFreePort() {
  return new Promise((resolve) => {
    const srv = net.createServer();
    srv.listen(0, () => {
      const port = srv.address().port;
      srv.close(() => resolve(port));
    });
  });
}

function createWindow(port) {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 900,
    minHeight: 600,
    backgroundColor: '#0f1115',
    title: 'BT 聚合搜索',
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  mainWindow.loadURL(`http://localhost:${port}/`);

  // 外部链接（如磁力调起）交给系统默认程序处理
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    require('electron').shell.openExternal(url);
    return { action: 'deny' };
  });

  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

// 单实例：重复双击只聚焦已有窗口，不再起第二个后端
if (!app.requestSingleInstanceLock()) {
  app.quit();
}

app.on('second-instance', () => {
  if (mainWindow) {
    if (mainWindow.isMinimized()) mainWindow.restore();
    mainWindow.focus();
  }
});

app.whenReady().then(async () => {
  const port = await getFreePort();
  try {
    server = start(port);
    await new Promise((resolve, reject) => {
      if (server.listening) return resolve();
      server.once('listening', resolve);
      server.once('error', reject);
    });
  } catch (e) {
    dialog.showErrorBox('启动失败', `后端无法启动：\n${e.message}`);
    app.quit();
    return;
  }
  createWindow(port);
});

// 关掉最后一个窗口即退出整个应用（后端随进程一起被回收）
app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

app.on('activate', () => {
  if (BrowserWindow.getAllWindows().length === 0 && server) {
    createWindow(server.address().port);
  }
});

app.on('before-quit', () => {
  if (server) {
    server.close();
    server = null;
  }
});
