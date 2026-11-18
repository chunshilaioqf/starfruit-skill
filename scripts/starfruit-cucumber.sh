#!/usr/bin/env bash
# starfruit × cucumber（黄瓜）可选集成：把节点进程注册进本机 cucumber 统一管理。
#
# 用法:
#   scripts/starfruit-cucumber.sh list                     # 列出 cucumber 进程（只读）
#   scripts/starfruit-cucumber.sh add <id> <名称> <启动命令> [工作目录]
#   scripts/starfruit-cucumber.sh start <id>               # 启动已注册进程
#   scripts/starfruit-cucumber.sh stop <id>                # 停止已注册进程
#
# 说明:
#   · 完全可选：不装 cucumber 不影响 starfruit 任何功能；
#   · add 会修改 cucumber 配置，执行前须征得用户同意（交互确认或 --yes）；
#   · 端口与令牌自动读取 cucumber 配置（macOS: ~/Library/Application Support/cucumber/config.toml，
#     其它: ~/.config/cucumber/config.toml）；
#   · 本脚本绝不删除（DELETE）任何 cucumber 进程配置。
set -u

read_cfg() {
  python3 - <<'PYEOF'
import os, re, sys

home = os.environ.get("HOME") or os.environ.get("USERPROFILE") or "."
paths = [
    os.path.join(home, "Library", "Application Support", "cucumber", "config.toml"),
    os.path.join(home, ".config", "cucumber", "config.toml"),
]
text = None
for p in paths:
    if os.path.exists(p):
        with open(p, "r", encoding="utf-8") as f:
            text = f.read()
        break
if text is None:
    sys.exit(1)
port, token = "", ""
try:
    import tomllib
    cfg = tomllib.loads(text)
    def find(d):
        global port, token
        for k, v in d.items():
            if isinstance(v, dict):
                find(v)
            elif k == "http_port":
                port = str(v)
            elif k == "auth_token":
                token = str(v)
    find(cfg)
except ImportError:
    m = re.search(r'^\s*http_port\s*=\s*(\d+)', text, re.M)
    port = m.group(1) if m else ""
    m = re.search(r'^\s*auth_token\s*=\s*"([^"]+)"', text, re.M)
    token = m.group(1) if m else ""
if not port:
    sys.exit(1)
print(port)
print(token)
PYEOF
}

CFG="$(read_cfg)" || {
  echo "[cucumber] 未找到 cucumber 配置（未安装或未运行）。此集成为可选，可忽略。" >&2
  exit 1
}
PORT="$(printf '%s\n' "$CFG" | sed -n 1p)"
TOKEN="$(printf '%s\n' "$CFG" | sed -n 2p)"
BASE="http://127.0.0.1:$PORT"

api() { # api <METHOD> <PATH> [JSON_BODY]
  local method="$1" path="$2" body="${3:-}"
  if [ -n "$body" ]; then
    curl -sS -m 8 -X "$method" -H "Authorization: Bearer $TOKEN" \
      -H "Content-Type: application/json" -d "$body" "$BASE$path"
  else
    curl -sS -m 8 -X "$method" -H "Authorization: Bearer $TOKEN" "$BASE$path"
  fi
}

fallback_ui() {
  echo "[cucumber] 当前 cucumber 版本不支持该 API 操作，请在 cucumber「对外接口/进程」页面手动完成：" >&2
  echo "  $1" >&2
  exit 1
}

cmd="${1:-}"
case "$cmd" in
  list)
    api GET /api/processes || fallback_ui "查看进程列表"
    echo
    ;;
  add)
    shift
    YES=0
    if [ "${1:-}" = "--yes" ]; then YES=1; shift; fi
    ID="${1:-}"; NAME="${2:-}"; START="${3:-}"; WORKDIR="${4:-$PWD}"
    if [ -z "$ID" ] || [ -z "$NAME" ] || [ -z "$START" ]; then
      echo "用法: $0 add [--yes] <id> <名称> <启动命令> [工作目录]" >&2
      exit 2
    fi
    if [ "$YES" -ne 1 ]; then
      if [ -t 0 ]; then
        printf '[cucumber] 将把进程「%s」注册进 cucumber（修改其配置）。确认？[y/N] ' "$NAME"
        read -r ans
        case "$ans" in y|Y|yes) ;; *) echo "[cucumber] 已取消"; exit 0 ;; esac
      else
        echo "[cucumber] add 会修改 cucumber 配置：请先征得用户同意，再带 --yes 执行" >&2
        exit 2
      fi
    fi
    BODY="$(ID="$ID" NAME="$NAME" START="$START" WORKDIR="$WORKDIR" python3 -c '
import json, os
print(json.dumps({
    "id": os.environ["ID"], "name": os.environ["NAME"], "type": "generic",
    "start_command": os.environ["START"], "stop_command": "",
    "check_command": "", "work_dir": os.environ["WORKDIR"],
}, ensure_ascii=False))')"
    resp="$(api POST /api/processes "$BODY")" || fallback_ui "新增进程 $NAME（启动命令: $START）"
    if printf '%s' "$resp" | grep -qiE '"error"|"message":".*(not found|unsupported)|404|405'; then
      fallback_ui "新增进程 $NAME（启动命令: $START）"
    fi
    echo "[cucumber] 已注册: $resp"
    ;;
  start|stop)
    ID="${2:-}"
    if [ -z "$ID" ]; then
      echo "用法: $0 $cmd <id>" >&2
      exit 2
    fi
    resp="$(api POST "/api/processes/$ID/$cmd")" || fallback_ui "$cmd 进程 $ID"
    if printf '%s' "$resp" | grep -qiE '404|405|not found'; then
      fallback_ui "$cmd 进程 $ID"
    fi
    echo "[cucumber] $cmd: $resp"
    ;;
  *)
    echo "用法: $0 list|add|start|stop ..." >&2
    echo "  list                 列出 cucumber 进程（只读）" >&2
    echo "  add [--yes] <id> <名称> <启动命令> [工作目录]   注册节点进程（须用户同意）" >&2
    echo "  start <id>           启动已注册进程" >&2
    echo "  stop <id>            停止已注册进程" >&2
    exit 2
    ;;
esac