#!/usr/bin/env bash
# QuickLAN 开发启动脚本：自动检查并补齐缺失依赖，然后启动开发模式
set -euo pipefail

cd "$(dirname "$0")"

# 1. Node.js / npm 环境检查
echo "==> 检查 Node.js 环境"
if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then
    echo "错误: 未检测到 Node.js / npm，请先安装 Node.js（建议 v20+）"
    echo "Ubuntu: sudo apt-get install -y nodejs npm"
    exit 1
fi

# 2. npm 依赖：node_modules 缺失时自动下载
echo "==> 检查 npm 依赖"
if [ ! -d node_modules ]; then
    echo "node_modules 缺失，自动执行 npm install ..."
    npm install
fi

# 3. Rust 工具链检查
echo "==> 检查 Rust 工具链"
if ! command -v cargo >/dev/null 2>&1; then
    echo "错误: 未检测到 Rust 工具链（cargo）"
    echo "可通过 rustup 安装: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

# 4. Tauri Linux 系统库检查（pkg-config 探测）
echo "==> 检查 Tauri Linux 系统依赖"
missing_libs=""
for lib in webkit2gtk-4.1 gtk+-3.0 libsoup-3.0 javascriptcoregtk-4.1; do
    if ! pkg-config --exists "$lib" 2>/dev/null; then
        missing_libs="$missing_libs $lib"
    fi
done
if [ -n "$missing_libs" ]; then
    echo "缺少系统库:$missing_libs"
    echo "自动安装 Tauri 系统依赖（需要 sudo 密码）..."
    sudo apt-get update
    sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential \
        curl wget file libxdo-dev libssl-dev \
        libayatana-appindicator3-dev librsvg2-dev
fi

# 5. 启动开发模式（Rust 依赖由 cargo 首次构建时自动下载）
echo "==> 启动开发模式"
npm run app:dev
