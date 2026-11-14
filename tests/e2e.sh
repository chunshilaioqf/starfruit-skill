#!/usr/bin/env bash
# starfruit 端到端自测：主节点 + 三语言子节点 + 双模式报错 + 面板 API + wake + 离线队列 + 工作团队。
# 用法: bash tests/e2e.sh   （需已编译 master 与 nodes/rust；需 python3 与 node）
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$ROOT/starfruit-data/e2e"
PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  ✅ $1"; }
bad()  { FAIL=$((FAIL+1)); echo "  ❌ $1"; }
check(){ if eval "$2" >/dev/null 2>&1; then ok "$1"; else bad "$1"; fi }

MASTER="$ROOT/master/target/release/starfruit-master"
NODE_RS="$ROOT/nodes/rust/target/release/starfruit-node"
NODE_PY="$ROOT/nodes/python/starfruit_node.py"
NODE_JS="$ROOT/nodes/nodejs/starfruit-node.js"

rm -rf "$WORK"; mkdir -p "$WORK"
cleanup() {
  for f in "$WORK"/*.pid; do
    [ -f "$f" ] && kill "$(cat "$f")" 2>/dev/null
  done
  sleep 0.3
  for f in "$WORK"/*.pid; do
    [ -f "$f" ] && kill -9 "$(cat "$f")" 2>/dev/null
  done
}
trap cleanup EXIT

echo "── 1. 启动主节点（plain，hub 19501 / web 19502）──"
"$MASTER" --mode plain --hub-addr 127.0.0.1:19501 --web-addr 127.0.0.1:19502 \
  --data-dir "$WORK/plain" --manager-name manager \
  --wake-cmd "echo \"WAKE:\$SF_FROM:\$SF_TEXT\" >> '$WORK/plain/wake-cmd-out.txt'" \
  >"$WORK/master-plain.log" 2>&1 &
echo $! > "$WORK/master-plain.pid"
sleep 1
check "主节点日志出现监听行" "grep -q '消息枢纽监听' $WORK/master-plain.log"
check "面板 API /api/stats 可访问" "curl -sf http://127.0.0.1:19502/api/stats"
check "面板首页可访问" "curl -sf http://127.0.0.1:19502/ | grep -q starfruit"
check "面板首页含浏览器图标链接" "curl -sf http://127.0.0.1:19502/ | grep -q '/static/img/favicon-32.png'"
check "面板 favicon（PNG）可访问" "curl -sf -o /dev/null -w '%{content_type}' http://127.0.0.1:19502/static/img/favicon-32.png | grep -q image/png"
check "面板 logo（SVG）可访问" "curl -sf -o /dev/null -w '%{content_type}' http://127.0.0.1:19502/static/img/logo.svg | grep -q image/svg+xml"
check "面板 SSE /events 可访问" "curl -sf -m 2 http://127.0.0.1:19502/events | head -1 | grep -q 'data\|ping'"

echo "── 2. 三语言子节点接入 + 注册回执 ──"
python3 "$NODE_PY" --name worker-py --mode plain --server ws://127.0.0.1:19501 \
  --data-dir "$WORK/plain" \
  --wake-cmd "echo \"WAKE:\$SF_FROM:\$SF_TEXT\" >> '$WORK/plain/wake-cmd-out.txt'" \
  >"$WORK/worker-py.log" 2>&1 &
echo $! > "$WORK/worker-py.pid"
node "$NODE_JS" --name worker-js --mode plain --server ws://127.0.0.1:19501 \
  --data-dir "$WORK/plain" >"$WORK/worker-js.log" 2>&1 &
echo $! > "$WORK/worker-js.pid"
"$NODE_RS" --name worker-rs --mode plain --server ws://127.0.0.1:19501 \
  --data-dir "$WORK/plain" >"$WORK/worker-rs.log" 2>&1 &
echo $! > "$WORK/worker-rs.pid"
sleep 2
check "python 节点注册成功" "grep -q 'registered' $WORK/worker-py.log"
check "nodejs 节点注册成功" "grep -q 'registered' $WORK/worker-js.log"
check "rust 节点注册成功" "grep -q 'registered' $WORK/worker-rs.log"
check "面板 /api/nodes 显示 4 节点" "[ \$(curl -sf http://127.0.0.1:19502/api/nodes | grep -o '\"name\"' | wc -l) -ge 4 ]"

echo "── 3. 消息路由（outbox → 定向/广播 → inbox）──"
sleep 0.6
echo '{"to":"worker-py","type":"task","text":"e2e-task-1"}' >> "$WORK/plain/manager.outbox.jsonl"
echo '{"to":"all","type":"notice","text":"e2e-broadcast"}' >> "$WORK/plain/manager.outbox.jsonl"
sleep 1.5
check "python 节点收到定向 task" "grep -q e2e-task-1 $WORK/plain/worker-py.inbox.jsonl"
check "python 节点收到广播" "grep -q e2e-broadcast $WORK/plain/worker-py.inbox.jsonl"
check "nodejs 节点收到广播" "grep -q e2e-broadcast $WORK/plain/worker-js.inbox.jsonl"
check "rust 节点收到广播" "grep -q e2e-broadcast $WORK/plain/worker-rs.inbox.jsonl"
echo '{"to":"manager","type":"done","text":"e2e-done"}' >> "$WORK/plain/worker-js.outbox.jsonl"
sleep 1.2
check "manager 收到 nodejs 的 done" "grep -q e2e-done $WORK/plain/manager.inbox.jsonl"
check "面板历史含消息" "curl -sf 'http://127.0.0.1:19502/api/messages?limit=50' | grep -q e2e-task-1"

echo "── 4. wake 唤醒钩子 ──"
echo '{"to":"worker-py","type":"wake","text":"e2e-wake"}' >> "$WORK/plain/manager.outbox.jsonl"
sleep 1.5
check "wake.log 记录唤醒" "grep -q e2e-wake $WORK/plain/wake.log"
check "python 节点触发 wake-cmd" "grep -q 'WAKE:manager:e2e-wake' $WORK/plain/wake-cmd-out.txt"

echo "── 5. 离线队列（暂存 → 上线投递）──"
kill "$(cat "$WORK/worker-py.pid")" 2>/dev/null; sleep 0.6
echo '{"to":"worker-py","type":"chat","text":"e2e-queued"}' >> "$WORK/plain/manager.outbox.jsonl"
sleep 1
python3 "$NODE_PY" --name worker-py --mode plain --server ws://127.0.0.1:19501 \
  --data-dir "$WORK/plain" \
  --wake-cmd "echo \"WAKE:\$SF_FROM:\$SF_TEXT\" >> '$WORK/plain/wake-cmd-out.txt'" \
  >>"$WORK/worker-py.log" 2>&1 &
echo $! > "$WORK/worker-py.pid"
sleep 1.5
check "离线消息在重连后投递" "grep -q e2e-queued $WORK/plain/worker-py.inbox.jsonl"

echo "── 6. 加密模式不匹配必须向对方报错 ──"
"$MASTER" --mode tls --hub-addr 127.0.0.1:19511 --web-addr 127.0.0.1:19512 \
  --data-dir "$WORK/tls" --manager-name manager >"$WORK/master-tls.log" 2>&1 &
echo $! > "$WORK/master-tls.pid"
sleep 1.2
# 明文节点 → tls 主节点：应收到 STARFRUIT_ERR
python3 "$NODE_PY" --name mis-node --mode plain --server ws://127.0.0.1:19511 \
  --data-dir "$WORK/tls" >"$WORK/mis-1.log" 2>&1
check "明文连加密端：收到 STARFRUIT_ERR 报错" "grep -q 'STARFRUIT_ERR mode_mismatch' $WORK/mis-1.log"
# tls 节点 → 明文主节点：主节点向对方发出明文报错；客户端提示模式不匹配
python3 "$NODE_PY" --name mis-node2 --mode tls --server wss://127.0.0.1:19501 \
  --data-dir "$WORK/plain" >"$WORK/mis-2.log" 2>&1
check "加密连明文端：主节点向对方发出报错" "grep -q '模式不匹配，已向对方报错' $WORK/master-plain.log"
check "加密连明文端：客户端识别为模式不匹配" "grep -q 'TLS 握手失败' $WORK/mis-2.log"
# 注册帧 mode 声明不匹配（传输层一致但声明错）→ 回执 mode_mismatch
check "注册帧 mode 声明不匹配：回执 mode_mismatch" \
  "python3 $ROOT/tests/mode_check.py 127.0.0.1 19501 tls"

echo "── 7. tls 模式正常接入 ──"
python3 "$NODE_PY" --name worker-tls --mode tls --server wss://127.0.0.1:19511 \
  --data-dir "$WORK/tls" >"$WORK/worker-tls.log" 2>&1 &
echo $! > "$WORK/worker-tls.pid"
sleep 1.5
check "tls 节点注册成功" "grep -q 'registered' $WORK/worker-tls.log"

echo "── 8. 监听规则（禁止 0.0.0.0 + 启动说明）──"
check "拒绝监听 0.0.0.0（hub）" \
  "'$MASTER' --hub-addr 0.0.0.0:19699 --data-dir '$WORK/guard' 2>&1 | grep -q '不允许监听通配地址'"
check "拒绝监听 0.0.0.0（web）" \
  "'$MASTER' --web-addr 0.0.0.0:19698 --data-dir '$WORK/guard' 2>&1 | grep -q '不允许监听通配地址'"
"$MASTER" --hub-addr 127.0.0.1:19601 --web-addr 127.0.0.1:19602 \
  --manager-name leader --data-dir "$WORK/banner" >"$WORK/banner.log" 2>&1 &
echo $! > "$WORK/banner.pid"
sleep 1
check "启动说明含主节点名称" "grep -q '主节点名称 : leader' $WORK/banner.log"
check "启动说明含节点加入方式（三语言客户端）" "grep -q 'Python3 : starfruit_node.py' $WORK/banner.log"
check "启动说明含监听规则（禁止 0.0.0.0）" "grep -q '禁止监听 0.0.0.0' $WORK/banner.log"

echo "── 9. 部署模式与端口规则 ──"
check "local 模式拒绝非回环地址（须 127.0.0.1）" \
  "'$MASTER' --deploy local --hub-addr 10.1.2.3:19710 --data-dir '$WORK/guard' 2>&1 | grep -q '本机模式'"
check "lan 模式拒绝回环地址（须局域网 IP）" \
  "'$MASTER' --deploy lan --hub-addr 127.0.0.1:19711 --data-dir '$WORK/guard' 2>&1 | grep -q '局域网模式'"
check "public 模式必须启用 tls" \
  "'$MASTER' --deploy public --mode plain --hub-addr 127.0.0.1:19712 --data-dir '$WORK/guard' 2>&1 | grep -q '必须启用加密'"
check "端口低于 10001 被拒绝" \
  "'$MASTER' --hub-addr 127.0.0.1:9001 --data-dir '$WORK/guard' 2>&1 | grep -q '必须 ≥ 10001'"
"$MASTER" --hub-addr 127.0.0.1:19711 --web-addr 127.0.0.1:19712 \
  --data-dir "$WORK/port-a" >"$WORK/port-a.log" 2>&1 &
echo $! > "$WORK/port-a.pid"
sleep 1
"$MASTER" --hub-addr 127.0.0.1:19711 --web-addr 127.0.0.1:19721 \
  --data-dir "$WORK/port-b" >"$WORK/port-b.log" 2>&1 &
echo $! > "$WORK/port-b.pid"
sleep 1
check "端口被占用自动 +1（19711→19713）" "grep -q 'ws://127.0.0.1:19713' $WORK/port-b.log"

echo "── 10. JWT 令牌（主节点签发 + 子节点校验）──"
JWT_DIR="$WORK/jwt"
mkdir -p "$JWT_DIR"
TOKEN="$("$MASTER" --issue-token worker-jwt --data-dir "$JWT_DIR" 2>/dev/null | tail -1)"
check "主节点可签发 JWT 令牌（header.payload.signature）" \
  "[ \$(printf '%s' '$TOKEN' | awk -F. '{print NF}') -eq 3 ]"
"$MASTER" --require-jwt --hub-addr 127.0.0.1:19731 --web-addr 127.0.0.1:19732 \
  --data-dir "$JWT_DIR" --manager-name manager >"$WORK/master-jwt.log" 2>&1 &
echo $! > "$WORK/master-jwt.pid"
sleep 1
check "启动说明含 JWT 强制提示" "grep -q 'JWT 校验   : 已强制启用' $WORK/master-jwt.log"
python3 "$NODE_PY" --name worker-nojwt --mode plain --server ws://127.0.0.1:19731 \
  --data-dir "$JWT_DIR" >"$WORK/worker-nojwt.log" 2>&1 &
echo $! > "$WORK/worker-nojwt.pid"
sleep 1.5
check "无令牌连接被拒绝（unauthorized）" "grep -q 'JWT 令牌校验失败' $WORK/worker-nojwt.log"
python3 "$NODE_PY" --name worker-jwt --mode plain --server ws://127.0.0.1:19731 \
  --data-dir "$JWT_DIR" --token "$TOKEN" >"$WORK/worker-jwt.log" 2>&1 &
echo $! > "$WORK/worker-jwt.pid"
sleep 1.5
check "携带正确令牌注册成功" "grep -q 'registered' $WORK/worker-jwt.log"
BAD_TOKEN="$("$MASTER" --issue-token other-node --data-dir "$JWT_DIR" 2>/dev/null | tail -1)"
python3 "$NODE_PY" --name worker-jwt --mode plain --server ws://127.0.0.1:19731 \
  --data-dir "$JWT_DIR" --token "$BAD_TOKEN" >"$WORK/worker-bad.log" 2>&1 &
echo $! > "$WORK/worker-bad.pid"
sleep 1.5
check "借用他人令牌被拒绝（sub 不一致）" "grep -q 'JWT 令牌校验失败' $WORK/worker-bad.log"

echo "── 11. 公网模式强制 tls + jwt ──"
PUB_DIR="$WORK/jwt-pub"
mkdir -p "$PUB_DIR"
"$MASTER" --deploy public --mode tls --hub-addr 127.0.0.1:19741 --web-addr 127.0.0.1:19742 \
  --data-dir "$PUB_DIR" >"$WORK/master-public.log" 2>&1 &
echo $! > "$WORK/master-public.pid"
sleep 1.5
check "公网模式启动并强制 JWT" "grep -q '公网模式：已强制启用 JWT' $WORK/master-public.log"
TOKEN_PUB="$("$MASTER" --issue-token worker-pub --data-dir "$PUB_DIR" 2>/dev/null | tail -1)"
python3 "$NODE_PY" --name worker-pub --mode tls --server wss://127.0.0.1:19741 \
  --data-dir "$PUB_DIR" --token "$TOKEN_PUB" >"$WORK/worker-pub.log" 2>&1 &
echo $! > "$WORK/worker-pub.pid"
sleep 1.5
check "公网模式（tls）携带令牌注册成功" "grep -q 'registered' $WORK/worker-pub.log"

echo "── 12. 工作团队（--init-team / --team）与保活/唤醒桥脚本 ──"
TEAM_DIR="$WORK/team"; mkdir -p "$TEAM_DIR"
check "--init-team 写出团队配置模板" \
  "'$MASTER' --init-team '$TEAM_DIR/team.json' 2>&1 | grep -q '已写出工作团队配置模板'"
check "团队模板含成员定义（worker-a）" "grep -q 'worker-a' '$TEAM_DIR/team.json'"
"$MASTER" --hub-addr 127.0.0.1:19751 --web-addr 127.0.0.1:19752 \
  --data-dir "$TEAM_DIR" --team "$TEAM_DIR/team.json" >"$WORK/master-team.log" 2>&1 &
echo $! > "$WORK/master-team.pid"
sleep 1
check "加入提示词含脱离会话后台运行（--daemon）" "grep -q -- '--daemon' $WORK/master-team.log"
check "加入提示词含保活脚本提示" "grep -q 'starfruit-keepalive.sh' $WORK/master-team.log"
check "加入提示词含 hooks 唤醒桥提示" "grep -q 'starfruit-wake-bridge.sh' $WORK/master-team.log"
check "提示词已落盘 team-prompts.md" "[ -f '$TEAM_DIR/team-prompts.md' ]"
check "面板 /api/team 返回成员 JSON（含 prompt）" "curl -sf http://127.0.0.1:19752/api/team | grep -q '\"prompt\"'"
check "面板 /api/team 含成员 worker-a" "curl -sf http://127.0.0.1:19752/api/team | grep -q 'worker-a'"
check "面板 /team 页面含一键复制脚本" "curl -sf http://127.0.0.1:19752/team | grep -q 'starfruitCopy'"
check "面板 /team-prompts.md 可访问（含成员）" "curl -sf http://127.0.0.1:19752/team-prompts.md | grep -q 'worker-a'"
"$MASTER" --hub-addr 127.0.0.1:19761 --web-addr 127.0.0.1:19762 \
  --data-dir "$WORK/team-remind" >"$WORK/master-remind.log" 2>&1 &
echo $! > "$WORK/master-remind.pid"
sleep 1
check "未配置团队时启动提醒用户可选配置" "grep -q '工作团队（可选）' $WORK/master-remind.log"
check "未配置团队时 /api/team 返回 team:null" "curl -sf http://127.0.0.1:19762/api/team | grep -q '\"team\":null'"
check "保活脚本：正常退出（exit 0）不重启" \
  "bash '$ROOT/scripts/starfruit-keepalive.sh' true 2>&1 | grep -q '正常退出'"
check "唤醒桥：缺少参数时提示用法" \
  "bash '$ROOT/scripts/starfruit-wake-bridge.sh' 2>&1 | grep -q '用法'"

echo
echo "════════ 结果: ✅ $PASS 通过 / ❌ $FAIL 失败 ════════"
[ "$FAIL" -eq 0 ]
