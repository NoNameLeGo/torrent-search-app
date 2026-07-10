@echo off
rem 停止 TorrentSearch 本地后端（关闭 3000 端口的 node 进程）
taskkill /fi "WINDOWTITLE eq TorrentSearch-Server*" >nul 2>nul
taskkill /f /im node.exe >nul 2>nul
echo 已尝试停止服务。
pause
