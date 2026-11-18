#!/usr/bin/env node
/**
 * starfruit-node.js —— starfruit 子节点（Node.js 实现，仅内置模块，Node ≥ 18）。
 *
 * 与 Rust/Python 版功能等价（协议 docs/protocol.md v2）：
 *   - 连接主节点（ws:// 明文 或 wss:// 加密，自签证书默认信任）
 *   - 注册（含 mode 声明；加密方式不对时服务端会向本节点报错）
 *   - 收消息 → inbox.jsonl；type=wake → 唤醒钩子（wake-cmd / wake-url / wake.log）
 *   - outbox.jsonl 追加式发消息（每行 {"to","type","text"}）
 *   - --daemon 后台运行（脱离终端，pidfile + 日志）
 *
 * 用法示例：
 *   node starfruit-node.js --name worker-js --mode plain \
 *       --wake-cmd 'echo "$SF_FROM: $SF_TEXT" >> /path/to/inbox.log'
 */

"use strict";

const net = require("net");
const tls = require("tls");
const crypto = require("crypto");
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");

const CURRENT_VERSION = "0.10.0";
const REPO_URL = "https://github.com/chunshilaioqf/starfruit-skill";
const CHANGELOG_URL = "https://raw.githubusercontent.com/chunshilaioqf/starfruit-skill/main/CHANGELOG.md";
const UPDATE_CHECK_INTERVAL_MS = 7 * 86400 * 1000;

function userDataRoot() {
  const home = process.env.HOME || process.env.USERPROFILE || ".";
  return path.join(home, ".starfruit");
}

// ─────────────────────────── 参数解析 ───────────────────────────

function parseArgs(argv) {
  const args = {
    server: "ws://127.0.0.1:10001",
    name: null,
    role: "worker",
    mode: "plain",
    token: null,
    dataDir: null,
    wakeCmd: null,
    wakeUrl: null,
    caCert: null,
    daemon: false,
    help: false,
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    const next = () => argv[++i];
    switch (a) {
      case "--server": args.server = next(); break;
      case "--name": args.name = next(); break;
      case "--role": args.role = next(); break;
      case "--mode": args.mode = next(); break;
      case "--token": args.token = next(); break;
      case "--data-dir": args.dataDir = next(); break;
      case "--wake-cmd": args.wakeCmd = next(); break;
      case "--wake-url": args.wakeUrl = next(); break;
      case "--ca-cert": args.caCert = next(); break;
      case "--daemon": args.daemon = true; break;
      case "-h": case "--help": args.help = true; break;
      default:
        console.error(`未知参数: ${a}`);
        process.exit(1);
    }
  }
  return args;
}

const HELP = `starfruit 子节点（Node.js 版）
用法: node starfruit-node.js --name <名字> [选项]

选项:
  --server <url>     主节点地址 ws:// 或 wss://（默认 ws://127.0.0.1:10001）
  --name <name>      节点名（全网唯一，必填；重名在线连接会被拒绝 name_conflict）
  --role <role>      角色标签（默认 worker）
  --mode plain|tls   加密模式，必须与主节点一致（默认 plain）
  --token <jwt>      JWT 令牌（主节点 --issue-token <本节点名> 签发；公网模式必填）
  --data-dir <dir>   运行数据目录（默认 ~/.starfruit/nodes/<name>）
  --wake-cmd <sh>    wake 唤醒钩子：shell 命令（SF_FROM/SF_TO/SF_TEXT/SF_TS）
  --wake-url <url>   wake 唤醒钩子：HTTP POST JSON
  --ca-cert <path>   TLS CA 证书（缺省信任自签）
  --daemon           后台运行
`;

function log(msg) {
  console.log(`[${new Date().toLocaleString("zh-CN", { hour12: false })}] [starfruit-node] ${msg}`);
}

// ─────────────────────────── WebSocket 客户端（RFC6455 最小实现） ───────────────────────────

class WsClient {
  /** @param {{host:string,port:number,tls:boolean,caCert:?string}} opt */
  constructor(opt) {
    this.opt = opt;
    this.buffer = Buffer.alloc(0);
    this.waiters = [];
    this.closed = false;
  }

  connect() {
    return new Promise((resolve, reject) => {
      const { host, port, tls: useTls, caCert } = this.opt;
      if (useTls) {
        // TLS 路径：若服务端实为明文，握手会失败并提示模式不匹配。
        const sock = tls.connect({
          host, port,
          ca: caCert ? [fs.readFileSync(caCert)] : undefined,
          rejectUnauthorized: !!caCert, // 缺省信任 LAN 自签证书
        });
        sock.on("error", (e) => reject(new Error(`TLS 握手失败（服务端可能运行在非加密模式 --mode plain）: ${e.message}`)));
        sock.on("secureConnect", () => {
          this.sock = sock;
          this._handshake().then(resolve, (e) => { sock.destroy(); reject(e); });
        });
      } else {
        const sock = net.connect({ host, port });
        sock.on("error", reject);
        sock.once("connect", () => {
          this.sock = sock;
          // 握手响应里若含 STARFRUIT_ERR（服务端加密方式不匹配报错），_handshake 会带出原文。
          this._handshake().then(resolve, (e) => { sock.destroy(); reject(e); });
        });
      }
    });
  }

  _onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    this._drain();
  }

  _drain() {
    while (true) {
      const frame = this._tryParseFrame();
      if (!frame) break;
      if (frame.op === 0x8) { this.closed = true; this._rejectAll(new Error("服务端关闭连接")); break; }
      if (frame.op === 0x9) { this._sendFrame(0xa, frame.payload); continue; }
      if (frame.op === 0xa) continue;
      this.fragments.push(frame.payload);
      if (frame.fin) {
        const text = Buffer.concat(this.fragments).toString("utf8");
        this.fragments = [];
        const w = this.waiters.shift();
        if (w) w.resolve(text);
        else this.inbox.push(text);
      }
    }
  }

  _tryParseFrame() {
    const buf = this.buffer;
    if (buf.length < 2) return null;
    const fin = (buf[0] & 0x80) !== 0;
    const op = buf[0] & 0x0f;
    const masked = (buf[1] & 0x80) !== 0;
    let len = buf[1] & 0x7f;
    let off = 2;
    if (len === 126) {
      if (buf.length < off + 2) return null;
      len = buf.readUInt16BE(off); off += 2;
    } else if (len === 127) {
      if (buf.length < off + 8) return null;
      len = Number(buf.readBigUInt64BE(off)); off += 8;
    }
    let mask = null;
    if (masked) {
      if (buf.length < off + 4) return null;
      mask = buf.slice(off, off + 4); off += 4;
    }
    if (buf.length < off + len) return null;
    const payload = Buffer.from(buf.slice(off, off + len));
    if (mask) for (let i = 0; i < payload.length; i++) payload[i] ^= mask[i % 4];
    this.buffer = buf.slice(off + len);
    return { fin, op, payload };
  }

  _sendFrame(op, payload) {
    const len = payload.length;
    let head;
    if (len < 126) head = Buffer.from([0x80 | op, 0x80 | len]);
    else if (len < 65536) { head = Buffer.alloc(4); head[0] = 0x80 | op; head[1] = 0x80 | 126; head.writeUInt16BE(len, 2); }
    else { head = Buffer.alloc(10); head[0] = 0x80 | op; head[1] = 0x80 | 127; head.writeBigUInt64BE(BigInt(len), 2); }
    const mask = crypto.randomBytes(4);
    const masked = Buffer.from(payload);
    for (let i = 0; i < masked.length; i++) masked[i] ^= mask[i % 4]; // 掩码循环 mask[i % 4]
    this.sock.write(Buffer.concat([head, mask, masked]));
  }

  sendText(text) {
    this._sendFrame(0x1, Buffer.from(text, "utf8"));
  }

  recv() {
    if (this.inbox.length) return Promise.resolve(this.inbox.shift());
    return new Promise((resolve, reject) => this.waiters.push({ resolve, reject }));
  }

  _rejectAll(err) {
    for (const w of this.waiters.splice(0)) w.reject(err);
  }

  _handshake(prelude = Buffer.alloc(0)) {
    return new Promise((resolve, reject) => {
      const key = crypto.randomBytes(16).toString("base64");
      const req =
        `GET / HTTP/1.1\r\nHost: ${this.opt.host}:${this.opt.port}\r\n` +
        "Upgrade: websocket\r\nConnection: Upgrade\r\n" +
        `Sec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`;
      this.fragments = [];
      this.inbox = [];
      this.handshakeBuf = Buffer.from(prelude);
      const onData = (chunk) => {
        this.handshakeBuf = Buffer.concat([this.handshakeBuf, chunk]);
        // 服务端加密方式不匹配时回的是明文错误行（非 HTTP 响应），优先识别。
        if (this.handshakeBuf.slice(0, 13).toString("latin1") === "STARFRUIT_ERR") {
          const nl = this.handshakeBuf.indexOf("\r\n");
          if (nl < 0) return;
          this.sock.off("data", onData);
          const line = this.handshakeBuf.slice(0, nl).toString("utf8").trim();
          this.sock.destroy();
          reject(new Error(`主节点拒绝连接：${line}`));
          return;
        }
        const idx = this.handshakeBuf.indexOf("\r\n\r\n");
        if (idx < 0) return;
        this.sock.off("data", onData);
        const head = this.handshakeBuf.slice(0, idx).toString("latin1");
        const rest = this.handshakeBuf.slice(idx + 4);
        if (!/^HTTP\/1\.1 101/.test(head)) {
          const detail = (head + "\n" + rest.toString("utf8")).trim();
          reject(new Error(`握手被拒绝：${detail.split("\r\n")[0]}${rest.length ? " " + rest.toString("utf8").trim() : ""}`));
          return;
        }
        const expect = crypto
          .createHash("sha1")
          .update(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11")
          .digest("base64");
        if (!head.includes(expect)) {
          reject(new Error("握手失败：Sec-WebSocket-Accept 不匹配"));
          return;
        }
        this.sock.on("data", (d) => this._onData(d));
        if (rest.length) this._onData(rest);
        resolve();
      };
      this.sock.on("data", onData);
      this.sock.on("close", () => {
        if (this.handshakeBuf) {
          const b = this.handshakeBuf;
          this.handshakeBuf = null;
          if (b.slice(0, 13).toString("latin1") === "STARFRUIT_ERR") {
            reject(new Error(`主节点拒绝连接：${b.toString("utf8").trim()}`));
            return;
          }
        }
        reject(new Error("握手失败：连接被关闭"));
      });
      this.sock.on("error", (e) => {
        this.handshakeBuf = null;
        reject(new Error(`连接失败：${e.message}`));
      });
      this.sock.write(req);
    });
  }

  close() {
    try { this._sendFrame(0x8, Buffer.alloc(0)); } catch { /* 忽略 */ }
    if (this.sock) this.sock.destroy();
  }
}

// ─────────────────────────── 每周更新检查 ───────────────────────────

function parseLatestVersion(changelog) {
  for (const line of changelog.split("\n")) {
    const m = line.trim().match(/^## \[([^\]]+)\]/);
    if (m && /^\d/.test(m[1])) return m[1];
  }
  return null;
}

function versionNewer(latest, current) {
  const parts = (s) => s.split(".").map((p) => parseInt(p, 10) || 0);
  const a = parts(latest), b = parts(current);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] || 0, y = b[i] || 0;
    if (x !== y) return x > y;
  }
  return false;
}

function fetchChangelog() {
  return new Promise((resolve) => {
    const req = require("https").get(CHANGELOG_URL, { timeout: 8000 }, (res) => {
      if (res.statusCode !== 200) { res.resume(); resolve(null); return; }
      let body = "";
      res.setEncoding("utf8");
      res.on("data", (c) => { body += c; });
      res.on("end", () => resolve(body));
    });
    req.on("timeout", () => { req.destroy(); resolve(null); });
    req.on("error", () => resolve(null));
  });
}

async function checkUpdateWeekly(dataDir) {
  const marker = path.join(dataDir, "update-check.json");
  let cached = {};
  try { cached = JSON.parse(fs.readFileSync(marker, "utf8")); } catch { /* 无缓存 */ }
  const now = Date.now();
  let latest = cached.latest || null;
  if (now - (cached.ts_ms || 0) >= UPDATE_CHECK_INTERVAL_MS) {
    const body = await fetchChangelog();
    const v = body ? parseLatestVersion(body) : null;
    if (v) {
      latest = v;
      try { fs.writeFileSync(marker, JSON.stringify({ ts_ms: now, latest })); } catch { /* 忽略 */ }
    }
  }
  if (latest && versionNewer(latest, CURRENT_VERSION)) {
    log(`发现新版本 ${latest}（当前 ${CURRENT_VERSION}）：请提醒用户更新 —— ${REPO_URL}（scripts/starfruit-update.sh，含数据迁移检查）`);
  }
}

// ─────────────────────────── 节点本体 ───────────────────────────

class Node {
  constructor(args) {
    this.args = args;
    this.dataDir = path.resolve(args.dataDir);
    fs.mkdirSync(this.dataDir, { recursive: true });
    this.name = args.name;
    this.inboxPath = path.join(this.dataDir, `${this.name}.inbox.jsonl`);
    this.outboxPath = path.join(this.dataDir, `${this.name}.outbox.jsonl`);
    this.wakeLog = path.join(this.dataDir, "wake.log");
  }

  triggerWake(v) {
    const rec = {
      from: v.from || "", to: v.to || "", text: v.text || "",
      ts: v.ts || "", ts_ms: Date.now(),
    };
    fs.appendFileSync(this.wakeLog, JSON.stringify(rec) + "\n");
    if (this.args.wakeCmd) {
      const child = spawn("/bin/sh", ["-c", this.args.wakeCmd], {
        env: { ...process.env, SF_FROM: rec.from, SF_TO: rec.to, SF_TEXT: rec.text, SF_TS: rec.ts },
        stdio: "ignore",
      });
      child.on("exit", (code) => { if (code !== 0) log(`wake-cmd 退出码 ${code}`); });
    }
    if (this.args.wakeUrl) {
      const body = JSON.stringify(rec);
      const u = new URL(this.args.wakeUrl);
      const mod = u.protocol === "https:" ? require("https") : require("http");
      const req = mod.request(u, { method: "POST", headers: { "Content-Type": "application/json" } });
      req.on("error", (e) => log(`wake-url POST 失败: ${e.message}`));
      req.end(body);
    }
  }

  drainOutbox() {
    try {
      const content = fs.readFileSync(this.outboxPath, "utf8");
      fs.writeFileSync(this.outboxPath, "");
      return content.split("\n").filter((l) => l.trim());
    } catch {
      return [];
    }
  }

  async runForever() {
    const BACKOFF = [2, 4, 8, 16, 30];
    let attempt = 0;
    for (;;) {
      try {
        await this.connectOnce();
        attempt = 0;
      } catch (e) {
        log(`连接中断: ${e.message}`);
        const msg = String(e.message);
        if (msg.includes("主节点拒绝连接") || msg.includes("mode_mismatch") ||
            msg.includes("TLS 握手失败") || msg.includes("STARFRUIT_ERR")) {
          log("加密模式不匹配，请与主节点保持相同的 --mode 后重试");
          process.exit(2);
        }
      }
      const delay = BACKOFF[Math.min(attempt, BACKOFF.length - 1)];
      attempt++;
      log(`${delay}s 后重连…`);
      await new Promise((r) => setTimeout(r, delay * 1000));
    }
  }

  async connectOnce() {
    const u = new URL(this.args.server.replace(/^ws/, "http"));
    const useTls = this.args.server.startsWith("wss");
    log(`连接 ${this.args.server}（mode=${this.args.mode}）`);
    await checkUpdateWeekly(this.dataDir);
    this.ws = new WsClient({
      host: u.hostname,
      port: Number(u.port || (useTls ? 443 : 80)),
      tls: useTls,
      caCert: this.args.caCert,
    });
    await this.ws.connect();

    this.ws.sendText(JSON.stringify({
      action: "register", from: this.name,
      role: this.args.role, mode: this.args.mode, ver: 2,
      token: this.args.token || "",
    }));

    const outboxTimer = setInterval(() => {
      for (const line of this.drainOutbox()) {
        let obj;
        try { obj = JSON.parse(line); } catch (e) { log(`outbox 行 JSON 解析失败: ${e.message}`); continue; }
        obj.from = this.name;
        obj.type = obj.type || "chat";
        obj.ts = obj.ts || new Date().toLocaleString("zh-CN", { hour12: false });
        obj.text = obj.text || "";
        this.ws.sendText(JSON.stringify(obj));
      }
    }, 500);

    try {
      for (;;) {
        const text = await this.ws.recv();
        this.onFrame(text);
      }
    } finally {
      clearInterval(outboxTimer);
      this.ws.close();
    }
  }

  onFrame(text) {
    let v;
    try { v = JSON.parse(text); } catch { log(`收到非 JSON 帧: ${text.slice(0, 80)}`); return; }

    if (v.system) {
      if (v.code === "mode_mismatch") {
        log(`加密模式不匹配（服务端报错）：${v.detail}`);
        process.exit(2);
      }
      if (v.code === "unauthorized") {
        log(`JWT 令牌校验失败（服务端报错）：${v.detail}`);
        process.exit(3);
      }
      if (v.code === "name_conflict") {
        log(`节点名冲突（服务端报错）：${v.detail}`);
        log("节点名称全网唯一：请更换 --name，或待同名旧连接断开后重试");
        process.exit(4);
      }
      log(`系统回执 ${v.code}: ${v.detail || ""}`);
      return;
    }

    fs.appendFileSync(this.inboxPath, text + "\n");
    const kind = v.type || "";
    log(`收到 ${v.from} → ${v.to} [${kind}] ${String(v.text || "").slice(0, 60)}`);
    if (kind === "wake") this.triggerWake(v);
  }
}

// ─────────────────────────── daemon / 入口 ───────────────────────────

function daemonize(dataDir, name) {
  const logPath = path.join(dataDir, `${name}.log`);
  const out = fs.openSync(logPath, "a");
  const child = spawn(process.execPath, process.argv.slice(1), {
    detached: true,
    stdio: ["ignore", out, out],
  });
  fs.writeFileSync(path.join(dataDir, `${name}.pid`), String(child.pid));
  child.unref();
  console.log(`starfruit-node ${name} 已后台启动（pid ${child.pid}），日志: ${logPath}`);
  process.exit(0);
}

function main() {
  const args = parseArgs(process.argv);
  if (args.help || !args.name) {
    console.log(HELP);
    process.exit(args.help ? 0 : 1);
  }
  if (!args.dataDir) {
    args.dataDir = path.join(userDataRoot(), "nodes", args.name);
  }
  if (args.daemon) {
    daemonize(path.resolve(args.dataDir), args.name);
    return;
  }
  log(`启动: name=${args.name} mode=${args.mode} server=${args.server}`);
  new Node(args).runForever().catch((e) => { log(`致命错误: ${e.message}`); process.exit(1); });
}

main();
