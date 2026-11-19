#!/usr/bin/env bash
# starfruit 独立 Cloudflare Tunnel 管理：与主节点生命周期完全解耦。
#
# 用法:
#   scripts/starfruit-tunnel.sh start [端口] [模式]   # 后台启动（默认指向 127.0.0.1:10001）
#   scripts/starfruit-tunnel.sh stop           # 显式停止（仅此命令会终止 Tunnel）
#   scripts/starfruit-tunnel.sh restart [端口] [模式] # 显式重启（公网地址会变化）
#   scripts/starfruit-tunnel.sh status         # 运行状态 + 公网地址
#   scripts/starfruit-tunnel.sh url            # 只输出公网地址
#
# 模式（第 3 参数或 STARFRUIT_TUNNEL_MODE，默认 auto）:
#   plain  转发 http://127.0.0.1:<port>   （主节点 --mode plain）
#   tls    转发 https://127.0.0.1:<port> + --no-tls-verify（主节点 --mode tls / public 自签证书）
#   auto   先探测主节点实际模式，再读 ~/.starfruit/master/setup.json，最后回退 plain
#   ⚠️ public 模式强制 tls：若用 plain 转发会 mode_mismatch 断连，务必 tls/auto。
#
# 说明:
#   · 数据目录 ~/.starfruit/tunnel（pid / 日志 / url 文件），跨平台统一；
#   · 主节点重启不影响本 Tunnel；主节点启动时读取 ~/.starfruit/tunnel/url 展示公网地址；
#   · cloudflared 崩溃时看门狗自动拉起（同一 Quick Tunnel 重连；地址可能变化）；
#   · 停止/重启只按本脚本记录的精确 PID 操作，绝不波及其它 cloudflared 进程。
set -u

TDIR="${HOME:-${USERPROFILE:-.}}/.starfruit/tunnel"
mkdir -p "$TDIR"
WD_PID="$TDIR/watchdog.pid"
CF_PID="$TDIR/cloudflared.pid"
URL_FILE="$TDIR/url"
LOG="$TDIR/cloudflared.log"
WD_LOG="$TDIR/watchdog.log"

pid_alive() { [ -f "$1" ] && kill -0 "$(cat "$1")" 2>/dev/null; }

read_url() { [ -f "$URL_FILE" ] && cat "$URL_FILE" || true; }

# 探测主节点运行模式：plain / tls。
# 原理：向 hub 端口发 TLS ClientHello——明文主节点会回明文 STARFRUIT_ERR 致 TLS 握手失败
# (curl exit 35)；tls 主节点握手成功（自签用 -k），返回 HTTP 层结果（exit 0/18/22/52/56）。
detect_mode() {
  local port="$1" rc
  if command -v curl >/dev/null 2>&1; then
    curl -sk --max-time 2 -o /dev/null "https://127.0.0.1:$port/" 2>/dev/null
    rc=$?
    case "$rc" in
      35) echo plain; return ;;
      0|18|22|52|56) echo tls; return ;;
    esac
  fi
  # 主节点未运行或探测不可判：读首启配置 setup.json 的 mode。
  local setup="${HOME:-${USERPROFILE:-.}}/.starfruit/master/setup.json"
  if [ -f "$setup" ] && command -v python3 >/dev/null 2>&1; then
    python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("mode","plain"))' "$setup" 2>/dev/null && return
  fi
  echo plain
}

# 由模式得出转发 scheme 与 cloudflared 附加参数。
scheme_of() { [ "$1" = "tls" ] && echo https || echo http; }
extra_flags_of() { [ "$1" = "tls" ] && echo "--no-tls-verify" || echo ""; }

cmd="${1:-}"
case "$cmd" in
  start|restart)
    port="${2:-${STARFRUIT_TUNNEL_PORT:-10001}}"
    mode="${3:-${STARFRUIT_TUNNEL_MODE:-auto}}"
    case "$mode" in
      plain|tls) ;;
      auto) mode="$(detect_mode "$port")" ;;
      *) echo "[tunnel] 无效模式: $mode（可选 plain|tls|auto）" >&2; exit 2 ;;
    esac
    if [ "$cmd" = "start" ] && pid_alive "$WD_PID"; then
      echo "[tunnel] 已在运行（watchdog pid $(cat "$WD_PID")），公网地址: $(read_url)"
      echo "[tunnel] 如需更换地址请显式执行: $0 restart"
      exit 0
    fi
    if [ "$cmd" = "restart" ]; then
      "$0" stop || true
    fi
    if ! command -v cloudflared >/dev/null 2>&1; then
      echo "[tunnel] 未找到 cloudflared，请先安装（brew install cloudflared）" >&2
      exit 1
    fi
    : > "$URL_FILE"
    nohup bash "$0" __watch "$port" "$mode" >> "$WD_LOG" 2>&1 &
    echo $! > "$WD_PID"
    echo "[tunnel] 已后台启动（watchdog pid $(cat "$WD_PID")），模式=$mode 指向 $(scheme_of "$mode")://127.0.0.1:$port"
    # 等待 Quick Tunnel 分配公网地址（最多 60s）。
    for _ in $(seq 1 60); do
      u="$(read_url)"
      if [ -n "$u" ]; then
        echo "[tunnel] 公网地址: $u"
        echo "[tunnel] 主节点重启不影响该地址；仅显式 stop/restart 才会变更"
        exit 0
      fi
      sleep 1
    done
    echo "[tunnel] 60s 内未取得公网地址，请查看日志: $LOG" >&2
    exit 1
    ;;
  stop)
    stopped=0
    # 先停看门狗（防止它重新拉起），再停 cloudflared；只动本脚本记录的 PID。
    for f in "$WD_PID" "$CF_PID"; do
      if pid_alive "$f"; then
        p="$(cat "$f")"
        kill "$p" 2>/dev/null && stopped=1
        sleep 0.3
        kill -0 "$p" 2>/dev/null && kill -9 "$p" 2>/dev/null
      fi
      rm -f "$f"
    done
    : > "$URL_FILE"
    if [ "$stopped" -eq 1 ]; then
      echo "[tunnel] 已停止（公网地址随之下线）"
    else
      echo "[tunnel] 未在运行"
    fi
    ;;
  status)
    if pid_alive "$WD_PID"; then
      echo "[tunnel] 运行中（watchdog pid $(cat "$WD_PID")）"
      echo "[tunnel] 公网地址: $(read_url)"
    else
      echo "[tunnel] 未运行"
      exit 1
    fi
    ;;
  url)
    read_url
    ;;
  __watch)
    # 内部看门狗：崩溃自动重启 cloudflared，并从日志解析公网地址写入 url 文件。
    port="${2:-10001}"
    mode="${3:-plain}"
    scheme="$(scheme_of "$mode")"
    extra="$(extra_flags_of "$mode")"
    BACKOFF=2
    while true; do
      echo "[tunnel-watch] $(date '+%F %T') 启动 cloudflared（mode=$mode）→ $scheme://127.0.0.1:$port"
      # 每次拉起前清空日志与旧地址，避免把上一轮的 URL 误当本轮地址。
      : > "$LOG"
      : > "$URL_FILE"
      # shellcheck disable=SC2086  # extra 为空或单个 --no-tls-verify，需词分割
      cloudflared tunnel --no-autoupdate --url "$scheme://127.0.0.1:$port" $extra >> "$LOG" 2>&1 &
      echo $! > "$CF_PID"
      # 地址解析（后台跟随日志，拿到即写 url 文件）。
      (
        for _ in $(seq 1 90); do
          u="$(grep -o 'https://[a-z0-9-]*\.trycloudflare\.com' "$LOG" 2>/dev/null | tail -1)"
          if [ -n "$u" ]; then printf '%s\n' "$u" > "$URL_FILE"; break; fi
          sleep 1
        done
      ) &
      wait "$(cat "$CF_PID")"
      code=$?
      rm -f "$CF_PID"
      echo "[tunnel-watch] cloudflared 退出（exit $code），${BACKOFF}s 后重启"
      sleep "$BACKOFF"
      BACKOFF=$(( BACKOFF < 30 ? BACKOFF * 2 : 30 ))
    done
    ;;
  *)
    echo "用法: $0 start|stop|restart|status|url [端口] [模式]" >&2
    echo "  start   后台启动独立 Tunnel（默认指向 127.0.0.1:10001；主节点重启不影响）" >&2
    echo "  stop    显式停止（仅本命令会终止 Tunnel）" >&2
    echo "  restart 显式重启（公网地址会变化）" >&2
    echo "  status  运行状态与公网地址" >&2
    echo "  url     只输出公网地址（~/.starfruit/tunnel/url）" >&2
    echo "  模式    plain|tls|auto（默认 auto；public/tls 主节点须 tls，用 https+--no-tls-verify）" >&2
    exit 2
    ;;
esac