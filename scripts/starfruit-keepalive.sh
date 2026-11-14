#!/usr/bin/env bash
# starfruit 保活（watchdog）：脱离 agent 会话长期监督任意节点进程，异常退出自动重启。
#
# 用法:
#   scripts/starfruit-keepalive.sh <节点可执行/脚本> [节点参数...]
#
# 示例:
#   scripts/starfruit-keepalive.sh python3 nodes/python/starfruit_node.py \
#     --name worker-a --mode plain --server ws://127.0.0.1:10001 --data-dir ./starfruit-data
#
# 说明:
#   · 建议在脱离会话的环境（nohup / systemd / 后台）中运行本脚本；
#   · 进程异常退出按 2s→30s 指数退避重启；正常退出（exit 0）不重启并结束；
#   · 亦可直接用 systemd（Restart=always + RestartSec）替代本脚本。
set -u

if [ $# -lt 1 ]; then
  echo "用法: $0 <节点可执行/脚本> [节点参数...]" >&2
  exit 2
fi

BACKOFF=2
while true; do
  echo "[keepalive] $(date '+%F %T') 启动: $*"
  "$@"
  code=$?
  if [ "$code" -eq 0 ]; then
    echo "[keepalive] 进程正常退出（exit 0），保活结束"
    exit 0
  fi
  echo "[keepalive] 进程异常退出（exit $code），${BACKOFF}s 后重启"
  sleep "$BACKOFF"
  BACKOFF=$(( BACKOFF < 30 ? BACKOFF * 2 : 30 ))
done
