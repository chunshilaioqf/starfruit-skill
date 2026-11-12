---
name: starfruit
description: "局域网多 Agent/多节点 WS 协同基建：唯一主节点（Rust，含只读 Web 可视化面板）+ 多种子节点（Rust/Python/Node.js），支持加密(wss)/非加密(ws)双模式、模式不匹配向对方报错、离线队列、消息历史与唤醒钩子。适用于需要多节点后台协同、任务派发/上报、消息流转可视化与唤醒对话对接的场景。"
metadata: { "openclaw": { "emoji": "⭐" } }
---

# starfruit（杨桃）—— 多节点 WS 协同 SKILL

星型拓扑消息枢纽：**一个主节点（管理者，全网唯一）+ 任意多个子节点（执行者）**。
一机可跑多节点；协议支持加密/非加密双模式；主节点带只读 Web 面板（消息流动画）。
所有节点都是**脱离 agent 会话的后台进程**，通过 **wake 钩子**唤醒对话（钩子由 agent 自行对接）。

方法论来源：`docs/skill-plan.md`；
协议契约：`docs/protocol.md`（v2）；命名由来：`docs/naming.md`。

## 何时使用

- 需要局域网内多执行者（人类 CLI、脚本、其他 agent）协同开发/作业，任务派发 + 上报；
- 需要可视化观察节点在线状态与消息流转、回看消息历史；
- 需要进程常驻后台、不依赖会话存活，且可被消息"唤醒对话"。

## 组件一览

| 组件 | 语言 | 路径 | 说明 |
|---|---|---|---|
| `starfruit-master` | Rust | `master/` | 主节点：路由/离线队列/历史/TLS/Web 面板/daemon |
| `starfruit-node` | Rust | `nodes/rust/` | 子节点（编译型，功能全） |
| `starfruit_node.py` | Python 3 | `nodes/python/` | 子节点（仅标准库，可直接运行） |
| `starfruit-node.js` | Node.js ≥18 | `nodes/nodejs/` | 子节点（仅内置模块，可直接运行） |

> ⚠️ **不提供二进制**：Rust 组件由用户安装本 SKILL 后**本地编译**（下文 Quickstart）。

## Quickstart

### 1. 编译（Rust 组件，用户本地执行）

```bash
cd <skill目录>/master && cargo build --release    # 产出 master/target/release/starfruit-master
cd <skill目录>/nodes/rust && cargo build --release # 产出 nodes/rust/target/release/starfruit-node
```

### 2. 启动主节点（唯一管理者）

三种部署模式（`--deploy`，默认 `local`）：

| `--deploy` | 监听约束 | 加密 / JWT |
|---|---|---|
| `local`（默认） | 只监听 `127.0.0.1` | tls / jwt 可选 |
| `lan` | 监听本机局域网 IP，**禁止 `0.0.0.0`** | tls / jwt 可选 |
| `public` | **禁止 `0.0.0.0`** 且必须指定具体 IP（配合 Cloudflare Tunnel 时监听 `127.0.0.1` 作为隧道指向） | **必须 `--mode tls`，且强制 JWT** |

```bash
# local 模式（默认，最安全）：仅监听 127.0.0.1，hub 127.0.0.1:10001 + 面板 127.0.0.1:10002
./master/target/release/starfruit-master --deploy local --mode plain --daemon

# 加密模式：首次启动生成自签证书（或 --cert/--key 指定）
./master/target/release/starfruit-master --deploy local --mode tls --daemon

# 局域网模式（lan）：监听本机局域网 IP（不要用 0.0.0.0）
./master/target/release/starfruit-master --deploy lan --mode plain --hub-addr 192.168.1.10:10001 --daemon

# 公网模式（public）：必须指定具体 IP + --mode tls + 强制 JWT（配合 Cloudflare Tunnel）
./master/target/release/starfruit-master --deploy public --hub-addr 127.0.0.1:10001 --mode tls --daemon
```

**主节点参数**：`--deploy <local|lan|public>`（默认 `local`）、`--mode <plain|tls>`、
`--hub-addr <IP:PORT>` / `--web-addr <IP:PORT>`（端口 **≥10001**，默认 hub `10001` / 面板 `10002`，
被占用时自动 +1）、`--cert` / `--key`、`--jwt-secret <密钥>`（缺省自动生成 `data-dir/jwt-secret`，权限 0600）、
`--jwt-ttl <秒>`（默认 86400）、`--require-jwt`、`--issue-token <节点名>`（签发后打印并退出）、`--daemon`。

**JWT 令牌**：令牌**只能由主节点签发**（`--issue-token <节点名>`，签发后打印并退出）；所有子节点连接时
**必须 `--token <令牌>`** 校验；令牌缺失或校验失败时回执 `unauthorized` 并断开连接（客户端退出码 3）。
算法 HS256，`iss=starfruit-master`，含 `sub` / `iat` / `exp`；`--require-jwt` 时强制启用。令牌**不可跨节点
借用**，过期即拒绝。

**启动成功后，会话中会打印启动说明**，包含：主节点名称、部署模式、运行模式、消息枢纽与面板地址、
数据目录，以及"其它执行节点如何加入"（三语言客户端命令）与"监听地址规则"。

**监听地址规则（务必遵守）**：

| 场景 | 监听地址 | 说明 |
|---|---|---|
| 仅本机多 agent 工具使用 | `127.0.0.1`（默认） | 对应 `--deploy local`，最安全，其它机器无法接入 |
| 局域网内多机协同 | 本机局域网 IP（如 `192.168.x.x`） | 对应 `--deploy lan`，供局域网其它机器加入 |
| 需要公网支持 | 本机地址 + **Cloudflare Tunnel** | 对应 `--deploy public`，由 Tunnel 暴露公网，**必须** `--mode tls` 且强制 JWT |

> ⛔ **禁止直接监听 `0.0.0.0`**：主节点启动时会**拒绝**通配地址（`0.0.0.0`/`::`）并提示正确做法，
> 避免无意中把服务暴露到所有网卡。明文（plain）监听非本机地址时会额外告警。
> 端口规则：所有模式端口必须 **≥10001**，默认 hub `10001` / 面板 `10002`，被占用时自动 +1。

启动后打开面板：`http://127.0.0.1:10002`（tls 模式为 `https://`，自签证书需浏览器信任）。
面板**无鉴权、全只读**：节点拓扑、消息流转（带动画）、消息历史、统计。

### 3. 启动子节点（任选一种实现，功能等价）

```bash
# Rust
./nodes/rust/target/release/starfruit-node --name worker-a --mode plain \
  --wake-cmd 'echo "$SF_FROM: $SF_TEXT" >> /path/to/agent-inbox.log'

# Python（零第三方依赖）
python3 nodes/python/starfruit_node.py --name worker-b --mode plain

# Node.js（零第三方依赖）
node nodes/nodejs/starfruit-node.js --name worker-c --mode plain
```

要点：
- **客户端选哪种实现由 agent 按所在机器环境决定**：有 Rust 工具链用 Rust 版；仅有 Python 用
  Python 版；仅有 Node 用 Node.js 版。三者为等价实现，协议完全相同，任选其一即可；
- `--name` **全网唯一**（同名重连顶掉旧连接）；一台机器可启动多个节点进程；
- `--mode` **必须与主节点一致**，否则连接时主节点会**向该节点报错**（`mode_mismatch`）；
- `--server ws://127.0.0.1:10001` 指定主节点地址（**默认 `ws://127.0.0.1:10001`**；跨机填主节点的局域网 IP）；
- `--token <令牌>`：主节点启用 JWT 时**必填**，缺失或校验失败回执 `unauthorized` 并断开（退出码 3）。

### 4. 唤醒钩子（agent 对接口子）

节点收到 `type=wake` 消息时触发其一：
- `--wake-cmd '<shell>'`：子进程执行，环境变量 `SF_FROM/SF_TO/SF_TEXT/SF_TS`；
- `--wake-url <url>`：POST JSON `{"from","to","text","ts"}`；
- 哨兵文件（默认）：`<data-dir>/wake.log` 追加 JSONL，agent 可 `tail -f` 唤醒会话。

**agent 对接方式（推荐）**：把 `--wake-cmd` 指向自己的唤醒入口（拉起会话的 CLI/脚本），
或监控 `wake.log`。节点不绑定任何具体 agent 实现。

## 协议摘要（详见 docs/protocol.md）

- 帧：JSON 文本，`{"from","to","type","text","ts"}`；`to` 支持 `"all"` 广播；
- 类型：`task / status / done / blocked / idle / notice / chat / wake`；
- 注册：首帧 `{"action":"register","from":"<name>","role":"worker|manager","mode":"plain|tls","ver":2}`；
- 回执：`registered / sent / queued / queued_delivered / broadcast_ok / no_such_peer / bad_msg / warn / mode_mismatch / unauthorized`；
- 离线队列：定向消息对端不在线暂存（每对端 200 条 FIFO），注册后自动投递；广播不补发。

## 后台运行（脱离会话）

- 所有节点支持 `--daemon`（脱离终端、写 `<data-dir>/<name>.pid` 与 `<name>.log`）；
- systemd unit 示例见 `docs/daemon.md`；nohup 方式亦可；
- 进程与任何 agent 会话生命周期无关；重启/守护由用户或 systemd 负责。

## 运维须知（来自实战经验总结）

- 清理测试实例**只按 PID/端口**，禁止宽模式 `pkill -f`（历史上误杀过生产 hub）；
- 执行者（子节点）**不要自建服务器**，只连主节点；
- 保持长连接（断线自动退避重连 2s→30s）；
- 契约（协议字段）变更需通知所有在途节点。

## 文件地图

```
SKILL.md                          ← 本文件
docs/protocol.md                  ← 协议 v2（双模式/报错语义/回执码）
docs/skill-plan.md                ← 方法论（含部署模式与 JWT）
docs/naming.md / daemon.md / workflow-optimized.md
master/                           ← 主节点（Rust，含 static/ 面板前端）
nodes/{rust,python,nodejs}/       ← 子节点三实现
reference/                        ← legacy ws-hub v0.2.0 等历史归档
```
