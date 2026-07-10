@echo off
rem ===========================================================
rem  TorrentSearch 启动器 —— 双击即可打开（无需命令行）
rem  会自动：① 起本地后端  ② 等端口就绪  ③ 打开浏览器
rem  若 3000 端口已在跑，则直接打开浏览器，不重复启动
rem ===========================================================
setlocal
cd /d "%~dp0"

rem 优先用 PATH 里的 node，找不到再退回 WorkBuddy 内置 node
set "NODE=node"
where node >nul 2>nul || set "NODE=C:\Users\LeGo\.workbuddy\binaries\node\versions\22.22.2\node.exe"

rem 已运行？直接打开浏览器
powershell -NoProfile -Command "try{(Invoke-WebRequest -Uri http://localhost:3000 -UseBasicParsing -TimeoutSec 2).StatusCode;exit 0}catch{exit 1}" >nul 2>nul
if %errorlevel%==0 (
  start "" http://localhost:3000
  goto :eof
)

rem 启动后端（最小化窗口，可随时关闭来停止服务）
start /min "TorrentSearch-Server" cmd /k "%NODE% server.js"

rem 等待端口就绪（最多 ~20 秒）
set /a tries=0
:wait
powershell -NoProfile -Command "try{(Invoke-WebRequest -Uri http://localhost:3000 -UseBasicParsing -TimeoutSec 2).StatusCode;exit 0}catch{exit 1}" >nul 2>nul
if %errorlevel%==0 goto :open
timeout /t 1 >nul
set /a tries+=1
if %tries% lss 20 goto :wait

:open
start "" http://localhost:3000
endlocal
