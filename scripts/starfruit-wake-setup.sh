#!/usr/bin/env bash
# starfruit 被动唤醒自动对接：把唤醒桥注册为当前 agent 工具的常驻钩子。
#
# 用法:
#   scripts/starfruit-wake-setup.sh <wake.log 路径> [唤醒命令]
#
# 行为:
#   1. 检测本机 agent 工具并为它注册"会话启动即拉起唤醒桥"的钩子（幂等，可重复执行）：
#      · Claude Code：合并 ~/.claude/settings.json 的 hooks.SessionStart；
#      · 其它 agent：打印接入说明（把本脚本或 wake-bridge 配成会话启动钩子即可）。
#   2. 立即后台常驻一个唤醒桥（nohup，pid 记录在 wake.log 同目录 wake-bridge.pid）。
#
# 唤醒命令优先级: 第 2 参数 > $STARFRUIT_WAKE_CMD > 检测到 claude CLI 时用
#   `claude -p "starfruit 唤醒（来自 $SF_FROM）：$SF_TEXT"`（headless 恢复工作）。
# 未提供唤醒命令时桥只记录事件，不调用 agent。
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BRIDGE="$ROOT/scripts/starfruit-wake-bridge.sh"

LOG="${1:-}"
if [ -z "$LOG" ]; then
  echo "用法: $0 <wake.log 路径> [唤醒命令]" >&2
  echo "示例: $0 ~/.starfruit/nodes/worker-a/wake.log" >&2
  exit 2
fi
mkdir -p "$(dirname "$LOG")"
[ -f "$LOG" ] || : >> "$LOG"
LOG="$(cd "$(dirname "$LOG")" && pwd)/$(basename "$LOG")"

WAKE_CMD="${2:-${STARFRUIT_WAKE_CMD:-}}"
if [ -z "$WAKE_CMD" ] && command -v claude >/dev/null 2>&1; then
  WAKE_CMD='claude -p "starfruit 唤醒（来自 $SF_FROM）：$SF_TEXT"'
  echo "[wake-setup] 检测到 claude CLI，默认唤醒命令: $WAKE_CMD"
fi

BRIDGE_PID="$(dirname "$LOG")/wake-bridge.pid"
BRIDGE_LOG="$(dirname "$LOG")/wake-bridge.log"

start_bridge() {
  if [ -f "$BRIDGE_PID" ] && kill -0 "$(cat "$BRIDGE_PID")" 2>/dev/null; then
    echo "[wake-setup] 唤醒桥已在运行（pid $(cat "$BRIDGE_PID")）"
    return 0
  fi
  nohup env STARFRUIT_WAKE_CMD="$WAKE_CMD" bash "$BRIDGE" "$LOG" >> "$BRIDGE_LOG" 2>&1 &
  echo $! > "$BRIDGE_PID"
  echo "[wake-setup] 唤醒桥已后台常驻（pid $(cat "$BRIDGE_PID")），监听 $LOG"
}

# ── Claude Code：合并 SessionStart 钩子（python3 幂等合并，不覆盖既有配置）──
setup_claude() {
  local settings="${HOME:-.}/.claude/settings.json"
  [ -d "${HOME:-.}/.claude" ] || return 1
  HOOK_CMD="nohup env STARFRUIT_WAKE_CMD=$(printf '%q' "$WAKE_CMD") bash $(printf '%q' "$BRIDGE") $(printf '%q' "$LOG") >> $(printf '%q' "$BRIDGE_LOG") 2>&1 &"
  SETTINGS="$settings" HOOK_CMD="$HOOK_CMD" LOG="$LOG" python3 - <<'PYEOF'
import json, os, sys

path = os.environ["SETTINGS"]
hook_cmd = os.environ["HOOK_CMD"]
log = os.environ["LOG"]
cfg = {}
if os.path.exists(path):
    try:
        with open(path, "r", encoding="utf-8") as f:
            cfg = json.load(f)
    except (OSError, json.JSONDecodeError):
        print(f"[wake-setup] {path} 解析失败，跳过钩子合并（不覆盖原文件）", file=sys.stderr)
        sys.exit(0)
hooks = cfg.setdefault("hooks", {})
session = hooks.setdefault("SessionStart", [])
# 幂等：已有指向同一 wake.log 的 starfruit 唤醒桥钩子则跳过。
for group in session:
    for h in group.get("hooks", []):
        c = h.get("command", "")
        if "starfruit-wake-bridge.sh" in c and log in c:
            print("[wake-setup] Claude Code SessionStart 钩子已存在，跳过")
            sys.exit(0)
session.append({"hooks": [{"type": "command", "command": hook_cmd}]})
with open(path, "w", encoding="utf-8") as f:
    json.dump(cfg, f, ensure_ascii=False, indent=2)
print(f"[wake-setup] 已注册 Claude Code SessionStart 钩子 → {path}")
PYEOF
  return 0
}

if setup_claude; then
  :
else
  echo "[wake-setup] 未检测到可自动对接的 agent（如 Claude Code）。"
  echo "[wake-setup] 其它 agent：请把下面命令配置为会话启动/常驻钩子——"
  echo "  nohup env STARFRUIT_WAKE_CMD='<你的唤醒命令>' bash $BRIDGE $LOG >> $BRIDGE_LOG 2>&1 &"
fi

start_bridge
echo "[wake-setup] 完成：节点收到 type=wake 消息即触发唤醒（日志 $BRIDGE_LOG）"