#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""starfruit_node.py —— starfruit 子节点（Python 实现，仅标准库）。

与 Rust/Node.js 版功能等价（协议 docs/protocol.md v2）：
  - 连接主节点（ws:// 明文 或 wss:// 加密，自签证书默认信任）
  - 注册（含 mode 声明；加密方式不对时服务端会向本节点报错）
  - 收消息 → inbox.jsonl；type=wake → 唤醒钩子（wake-cmd / wake-url / wake.log）
  - outbox.jsonl 追加式发消息（每行 {"to","type","text"}）
  - --daemon 后台运行（脱离终端，pidfile + 日志）

用法示例：
  python3 starfruit_node.py --name worker-py --mode plain \
      --wake-cmd 'echo "$SF_FROM: $SF_TEXT" >> /path/to/inbox.log'
"""

import argparse
import base64
import hashlib
import json
import os
import socket
import ssl
import struct
import sys
import threading
import time
import urllib.request

RECV_MAX = 2 * 1024 * 1024
BACKOFF = [2, 4, 8, 16, 30]

CURRENT_VERSION = "0.10.0"
REPO_URL = "https://github.com/chunshilaioqf/starfruit-skill"
CHANGELOG_URL = "https://raw.githubusercontent.com/chunshilaioqf/starfruit-skill/main/CHANGELOG.md"
UPDATE_CHECK_INTERVAL_MS = 7 * 86400 * 1000


def log(msg):
    ts = time.strftime("%Y-%m-%d %H:%M:%S")
    print(f"[{ts}] [starfruit-node] {msg}", flush=True)


def user_data_root():
    home = os.environ.get("HOME") or os.environ.get("USERPROFILE") or "."
    return os.path.join(home, ".starfruit")


# ─────────────────────────── 每周更新检查 ───────────────────────────

def parse_latest_version(changelog):
    """首个 `## [x.y.z]`（跳过 Unreleased）。"""
    for line in changelog.splitlines():
        line = line.strip()
        if line.startswith("## [") and "]" in line:
            ver = line[4:].split("]", 1)[0]
            if ver[:1].isdigit():
                return ver
    return None


def version_newer(latest, current):
    def parts(s):
        return [int(p) if p.strip().isdigit() else 0 for p in s.split(".")]
    return parts(latest) > parts(current)


def check_update_weekly(data_dir):
    """每周一次比对线上 CHANGELOG；失败静默（不推进时间戳，下次连接再试）。"""
    marker = os.path.join(data_dir, "update-check.json")
    cached = {}
    try:
        with open(marker, "r", encoding="utf-8") as f:
            cached = json.load(f)
    except (OSError, json.JSONDecodeError):
        pass
    now = int(time.time() * 1000)
    latest = cached.get("latest")
    if now - int(cached.get("ts_ms", 0)) >= UPDATE_CHECK_INTERVAL_MS:
        try:
            with urllib.request.urlopen(CHANGELOG_URL, timeout=8) as resp:
                body = resp.read().decode("utf-8", "replace")
            v = parse_latest_version(body)
            if v:
                latest = v
                with open(marker, "w", encoding="utf-8") as f:
                    json.dump({"ts_ms": now, "latest": latest}, f)
        except Exception:  # noqa: BLE001 —— 更新检查失败不影响节点运行
            pass
    if latest and version_newer(latest, CURRENT_VERSION):
        log(f"发现新版本 {latest}（当前 {CURRENT_VERSION}）：请提醒用户更新 —— "
            f"{REPO_URL}（scripts/starfruit-update.sh，含数据迁移检查）")


# ─────────────────────────── WebSocket 客户端（RFC6455 最小实现） ───────────────────────────

class WsClient:
    """标准库裸实现：握手 + 掩码帧。掩码必须循环 mask[i % 4]（实战教训）。"""

    def __init__(self, host, port, path, tls=False, ca_cert=None):
        self.host, self.port, self.path = host, port, path
        raw = socket.create_connection((host, port), timeout=30)
        if tls:
            if ca_cert:
                ctx = ssl.create_default_context(cafile=ca_cert)
            else:
                ctx = ssl._create_unverified_context()  # LAN 自签证书默认信任
            try:
                raw = ctx.wrap_socket(raw, server_hostname=host)
            except ssl.SSLError as e:
                raw.close()
                raise RuntimeError(
                    f"TLS 握手失败（服务端可能运行在非加密模式 --mode plain）: {e}"
                ) from e
        self.sock = raw
        self._handshake()

    def _handshake(self):
        key = base64.b64encode(os.urandom(16)).decode()
        req = (
            f"GET {self.path} HTTP/1.1\r\n"
            f"Host: {self.host}:{self.port}\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n\r\n"
        )
        self.sock.sendall(req.encode())
        buf = b""
        while b"\r\n\r\n" not in buf:
            # 服务端加密方式不匹配时回的是明文错误行（非 HTTP 响应），优先识别。
            if buf.startswith(b"STARFRUIT_ERR") and b"\r\n" in buf:
                line = buf.split(b"\r\n", 1)[0].decode("utf-8", "replace").strip()
                self.sock.close()
                raise RuntimeError(f"主节点拒绝连接：{line}")
            chunk = self.sock.recv(4096)
            if not chunk:
                if buf.startswith(b"STARFRUIT_ERR"):
                    line = buf.decode("utf-8", "replace").strip()
                    self.sock.close()
                    raise RuntimeError(f"主节点拒绝连接：{line}")
                raise RuntimeError("握手失败：连接被关闭")
            buf += chunk
        head, rest = buf.split(b"\r\n\r\n", 1)
        status = head.split(b"\r\n", 1)[0].decode("latin1")
        if " 101 " not in f" {status} ":
            detail = rest.decode("utf-8", "replace").strip() or status
            raise RuntimeError(f"握手被拒绝：{detail}")
        accept = base64.b64encode(
            hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
        ).decode()
        if accept.encode() not in head:
            raise RuntimeError("握手失败：Sec-WebSocket-Accept 不匹配")
        self._rest = rest  # 可能粘包的剩余字节

    def _recv_exact(self, n):
        buf = self._rest or b""
        self._rest = b""
        while len(buf) < n:
            chunk = self.sock.recv(n - len(buf))
            if not chunk:
                raise RuntimeError("连接已关闭")
            buf += chunk
        return buf

    def _send_frame(self, opcode, payload):
        mask = os.urandom(4)
        head = bytes([0x80 | opcode])
        n = len(payload)
        if n < 126:
            head += bytes([0x80 | n])
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            head += bytes([0x80 | 127]) + struct.pack(">Q", n)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self.sock.sendall(head + mask + masked)

    def send_text(self, text):
        self._send_frame(0x1, text.encode("utf-8"))

    def ping(self):
        self._send_frame(0x9, b"ping")

    def recv_text(self):
        """收一条文本帧（自动处理 ping/pong/close 与分片）。"""
        parts = []
        while True:
            h = self._recv_exact(2)
            fin_op = h[0]
            opcode = fin_op & 0x0F
            fin = bool(fin_op & 0x80)
            masked = bool(h[1] & 0x80)
            n = h[1] & 0x7F
            if n == 126:
                (n,) = struct.unpack(">H", self._recv_exact(2))
            elif n == 127:
                (n,) = struct.unpack(">Q", self._recv_exact(8))
            mask = self._recv_exact(4) if masked else None
            payload = self._recv_exact(n) if n else b""
            if mask:
                payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))

            if opcode == 0x8:  # close
                raise RuntimeError("服务端关闭连接")
            if opcode == 0x9:  # ping → pong
                self._send_frame(0xA, payload)
                continue
            if opcode == 0xA:  # pong
                continue
            parts.append(payload)
            if fin:
                return b"".join(parts).decode("utf-8", "replace")

    def close(self):
        try:
            self._send_frame(0x8, b"")
        except OSError:
            pass
        self.sock.close()


# ─────────────────────────── 节点本体 ───────────────────────────

class Node:
    def __init__(self, args):
        self.args = args
        self.data_dir = os.path.abspath(args.data_dir)
        os.makedirs(self.data_dir, exist_ok=True)
        self.name = args.name
        self.mode = args.mode
        self.inbox = os.path.join(self.data_dir, f"{self.name}.inbox.jsonl")
        self.outbox = os.path.join(self.data_dir, f"{self.name}.outbox.jsonl")
        self.wake_log = os.path.join(self.data_dir, "wake.log")
        self.ws = None
        self.send_lock = threading.Lock()

    # —— wake 唤醒钩子（agent 对接口子） ——
    def trigger_wake(self, v):
        rec = {
            "from": v.get("from", ""), "to": v.get("to", ""),
            "text": v.get("text", ""), "ts": v.get("ts", ""),
            "ts_ms": int(time.time() * 1000),
        }
        with open(self.wake_log, "a", encoding="utf-8") as f:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")

        if self.args.wake_cmd:
            env = dict(os.environ, SF_FROM=rec["from"], SF_TO=rec["to"],
                       SF_TEXT=rec["text"], SF_TS=rec["ts"])
            rc = os.spawnve(os.P_WAIT, "/bin/sh", ["sh", "-c", self.args.wake_cmd], env)
            if rc != 0:
                log(f"wake-cmd 退出码 {rc}")
        if self.args.wake_url:
            body = json.dumps(rec, ensure_ascii=False).encode()
            req = urllib.request.Request(
                self.args.wake_url, data=body,
                headers={"Content-Type": "application/json"}, method="POST")
            try:
                urllib.request.urlopen(req, timeout=10).read()
            except Exception as e:  # noqa: BLE001 —— 钩子失败不拖垮节点
                log(f"wake-url POST 失败: {e}")

    def append_inbox(self, text):
        with open(self.inbox, "a", encoding="utf-8") as f:
            f.write(text + "\n")

    def drain_outbox(self):
        try:
            with open(self.outbox, "r+", encoding="utf-8") as f:
                lines = f.readlines()
                f.seek(0)
                f.truncate()
        except FileNotFoundError:
            return []
        return [ln for ln in lines if ln.strip()]

    def send_json(self, obj):
        text = json.dumps(obj, ensure_ascii=False)
        with self.send_lock:
            self.ws.send_text(text)

    # —— 连接与主循环 ——
    def run_forever(self):
        attempt = 0
        while True:
            try:
                self.connect_once()
                attempt = 0
            except Exception as e:  # noqa: BLE001 —— 断线统一走退避重连
                log(f"连接中断: {e}")
                msg = str(e)
                if "主节点拒绝连接" in msg or "mode_mismatch" in msg or "TLS 握手失败" in msg:
                    log("加密模式不匹配，请与主节点保持相同的 --mode 后重试")
                    sys.exit(2)
            delay = BACKOFF[min(attempt, len(BACKOFF) - 1)]
            attempt += 1
            log(f"{delay}s 后重连…")
            time.sleep(delay)

    def connect_once(self):
        u = self.args.server.replace("ws://", "").replace("wss://", "")
        tls = self.args.server.startswith("wss")
        host, _, port = u.partition(":")
        port = int(port or ("443" if tls else "80"))
        log(f"连接 {self.args.server}（mode={self.mode}）")
        check_update_weekly(self.data_dir)
        self.ws = WsClient(host, port, "/", tls=tls, ca_cert=self.args.ca_cert)

        self.send_json({
            "action": "register", "from": self.name,
            "role": self.args.role, "mode": self.mode, "ver": 2,
            "token": self.args.token or "",
        })

        # outbox 轮询线程
        stop = threading.Event()

        def outbox_loop():
            while not stop.is_set():
                for line in self.drain_outbox():
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        obj = json.loads(line)
                    except json.JSONDecodeError as e:
                        log(f"outbox 行 JSON 解析失败: {e}")
                        continue
                    obj["from"] = self.name
                    obj.setdefault("type", "chat")
                    obj.setdefault("ts", time.strftime("%Y-%m-%d %H:%M:%S"))
                    obj["text"] = obj.get("text", "")
                    try:
                        self.send_json(obj)
                    except OSError as e:
                        log(f"outbox 发送失败: {e}")
                time.sleep(0.5)

        t = threading.Thread(target=outbox_loop, daemon=True)
        t.start()
        last_ping = time.time()
        try:
            while True:
                self.ws.sock.settimeout(20)
                try:
                    text = self.ws.recv_text()
                except socket.timeout:
                    if time.time() - last_ping > 25:
                        self.ws.ping()
                        last_ping = time.time()
                    continue
                self.ws.sock.settimeout(None)
                self.on_frame(text)
        finally:
            stop.set()
            self.ws.close()

    def on_frame(self, text):
        try:
            v = json.loads(text)
        except json.JSONDecodeError:
            log(f"收到非 JSON 帧: {text[:80]}")
            return

        if v.get("system"):
            code = v.get("code", "")
            if code == "mode_mismatch":
                log(f"加密模式不匹配（服务端报错）：{v.get('detail')}")
                sys.exit(2)
            if code == "unauthorized":
                log(f"JWT 令牌校验失败（服务端报错）：{v.get('detail')}")
                sys.exit(3)
            if code == "name_conflict":
                log(f"节点名冲突（服务端报错）：{v.get('detail')}")
                log("节点名称全网唯一：请更换 --name，或待同名旧连接断开后重试")
                sys.exit(4)
            log(f"系统回执 {code}: {v.get('detail', '')}")
            return

        self.append_inbox(text)
        kind = v.get("type", "")
        log(f"收到 {v.get('from')} → {v.get('to')} [{kind}] {str(v.get('text', ''))[:60]}")
        if kind == "wake":
            self.trigger_wake(v)


# ─────────────────────────── daemon / 入口 ───────────────────────────

def daemonize(data_dir, name):
    log_path = os.path.join(data_dir, f"{name}.log")
    if os.fork() > 0:
        print(f"starfruit-node {name} 已后台启动，日志: {log_path}")
        sys.exit(0)
    os.setsid()
    if os.fork() > 0:
        sys.exit(0)
    with open("/dev/null", "rb") as devnull:
        os.dup2(devnull.fileno(), 0)
    lf = open(log_path, "ab", buffering=0)
    os.dup2(lf.fileno(), 1)
    os.dup2(lf.fileno(), 2)
    with open(os.path.join(data_dir, f"{name}.pid"), "w", encoding="utf-8") as f:
        f.write(str(os.getpid()))


def main():
    ap = argparse.ArgumentParser(description="starfruit 子节点（Python 版）")
    ap.add_argument("--server", default="ws://127.0.0.1:10001",
                    help="主节点地址 ws:// 或 wss://（默认 ws://127.0.0.1:10001）")
    ap.add_argument("--name", required=True, help="节点名（全网唯一；重名在线连接会被拒绝 name_conflict）")
    ap.add_argument("--role", default="worker", help="角色标签（默认 worker）")
    ap.add_argument("--mode", default="plain", choices=["plain", "tls"],
                    help="加密模式，必须与主节点一致（默认 plain）")
    ap.add_argument("--token", default=None,
                    help="JWT 令牌（主节点 --issue-token <本节点名> 签发；公网模式必填）")
    ap.add_argument("--data-dir", default=None,
                    help="运行数据目录（默认 ~/.starfruit/nodes/<name>）")
    ap.add_argument("--wake-cmd", default=None, help="wake 唤醒钩子：shell 命令")
    ap.add_argument("--wake-url", default=None, help="wake 唤醒钩子：HTTP POST URL")
    ap.add_argument("--ca-cert", default=None, help="TLS CA 证书（缺省信任自签）")
    ap.add_argument("--daemon", action="store_true", help="后台运行")
    args = ap.parse_args()

    if args.data_dir is None:
        args.data_dir = os.path.join(user_data_root(), "nodes", args.name)
    if args.daemon:
        daemonize(os.path.abspath(args.data_dir), args.name)
    log(f"启动: name={args.name} mode={args.mode} server={args.server}")
    Node(args).run_forever()


if __name__ == "__main__":
    main()
