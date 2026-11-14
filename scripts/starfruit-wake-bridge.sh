#!/usr/bin/env bash
# starfruit 唤醒桥：复用 agent 的 hooks 机制，被动接收主节点任务并唤醒当前 agent 继续工作。
#
# 原理:
#   节点收到 type=wake 消息时会把一行 JSON 追加到 <data-dir>/wake.log（哨兵文件钩子）。
#   本脚本 tail -F 该文件，每读到一行即调用 $STARFRUIT_WAKE_CMD（你的 agent 唤醒入口），
#   并通过环境变量 SF_FROM / SF_TO / SF_TEXT / SF_TS 注入消息内容（与 --wake-cmd 语义一致）。
#
# 用法:
#   STARFRUIT_WAKE_CMD='<你的 agent 唤醒命令>' \
#     scripts/starfruit-wake-bridge.sh <wake.log 路径>
#
# 作为 agent hook 注册（推荐）:
#   将本脚本配置为 agent 的“会话启动钩子 / 常驻后台钩子”，随会话启动并在后台常驻；
#   收到主节点任务时由 STARFRUIT_WAKE_CMD 恢复/继续当前会话（示例：拉起会话 CLI 并注入提示词）。
set -u

LOG="${1:-}"
CMD="${STARFRUIT_WAKE_CMD:-}"

if [ -z "$LOG" ]; then
  echo "用法: STARFRUIT_WAKE_CMD='<唤醒命令>' $0 <wake.log 路径>" >&2
  exit 2
fi
if [ -z "$CMD" ]; then
  echo "[wake-bridge] 未设置 STARFRUIT_WAKE_CMD：仅打印唤醒事件，不调用 agent" >&2
fi

[ -f "$LOG" ] || : >> "$LOG"
echo "[wake-bridge] 监听 $LOG（Ctrl-C 退出）"

# -n0：只看新追加的行；-F：文件被重建也继续跟随。
tail -n0 -F "$LOG" | while IFS= read -r line; do
  [ -z "$line" ] && continue
  # 用 python3 解析 JSON 并做 shell 安全引用（shlex.quote），零第三方依赖。
  parsed="$(printf '%s' "$line" | python3 -c '
import sys, json, shlex
try:
    v = json.loads(sys.stdin.read() or "{}")
except Exception:
    v = {}
for k in ("from", "to", "text", "ts"):
    print(shlex.quote(str(v.get(k, ""))))
' 2>/dev/null)"
  # parsed 各字段已由 shlex.quote 安全引用，eval 恢复为参数是安全的。
  # shellcheck disable=SC2086
  eval "set -- $parsed"
  SF_FROM="${1:-}"; SF_TO="${2:-}"; SF_TEXT="${3:-}"; SF_TS="${4:-}"
  echo "[wake-bridge] 唤醒: from=$SF_FROM to=$SF_TO text=$SF_TEXT"
  if [ -n "$CMD" ]; then
    SF_FROM="$SF_FROM" SF_TO="$SF_TO" SF_TEXT="$SF_TEXT" SF_TS="$SF_TS" sh -c "$CMD"
  fi
done
