#!/usr/bin/env bash
# starfruit 更新：拉取最新代码 + 重新编译 + 数据迁移检查提示。
#
# 用法:
#   scripts/starfruit-update.sh [--no-build]
#
# 说明:
#   · 仓库默认 https://github.com/chunshilaioqf/starfruit-skill；
#   · 更新后需重启主节点与子节点生效（保活脚本会自动拉起，或手动重启）；
#   · 主节点启动时自动执行数据迁移检查（data-version 标记，~/.starfruit/master）；
#   · 本脚本不触碰任何运行中的进程，也不重启 Cloudflare Tunnel
#     （Tunnel 独立后台运行，主节点重启不影响公网地址）。
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

NO_BUILD=0
case "${1:-}" in
  -h|--help)
    echo "用法: $0 [--no-build]"
    echo "  拉取最新代码并重新编译；重启主节点/子节点生效（Tunnel 不受影响）"
    exit 0
    ;;
  --no-build) NO_BUILD=1 ;;
esac

OLD_VER="$(cat VERSION 2>/dev/null || echo '?')"
echo "[update] 当前版本: $OLD_VER"
echo "[update] 拉取最新代码（git pull --ff-only）…"
git pull --ff-only || { echo "[update] git pull 失败，请手动处理" >&2; exit 1; }
NEW_VER="$(cat VERSION 2>/dev/null || echo '?')"
echo "[update] 仓库版本: $NEW_VER"

if [ "$NO_BUILD" -eq 0 ]; then
  echo "[update] 编译主节点…"
  (cd master && cargo build --release) || exit 1
  echo "[update] 编译 Rust 子节点…"
  (cd nodes/rust && cargo build --release) || exit 1
fi

echo "[update] 完成。后续步骤："
echo "  1. 重启主节点（数据迁移检查将在启动时自动执行，日志出现 [migrate] 行）；"
echo "  2. 重启各子节点（若由 starfruit-keepalive.sh 守护，结束旧进程即自动拉起新版本）；"
echo "  3. Cloudflare Tunnel 无需重启（独立运行，公网地址不变）。"