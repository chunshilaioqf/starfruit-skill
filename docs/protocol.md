# starfruit 协议 v2 —— 局域网多节点 WS 协同协议

> 在 ws_protocol v1.1（本机 ZCode ↔ Trae 直连）基础上演进：
> ①星型拓扑扩展为**局域网多节点**（一机多节点、唯一主节点）；②支持**加密（TLS/wss）与
> 非加密（plain/ws）双模式运行**，模式不匹配必须**向对方报错**；③新增 `wake` 唤醒消息。
> 兼容保留 v1.1 的消息类型、回执码与离线队列语义。版本：**v2.0**（2026-11-12）。

---

## 1. 拓扑与角色

```
            ┌──────────┐
   worker ──┤          ├── worker        ← 局域网任意机器
            │ 主节点   │                 ← 全网唯一管理者（hub + Web 面板）
   worker ──│ (hub)    ├── worker        ← 同一台机器可跑多个节点（名字唯一）
            └──────────┘
```

| 角色 | 数量 | 职责 |
|---|---|---|
| **主节点 manager/hub** | 全网唯一 | 消息路由、离线队列、历史记录、Web 只读面板、唤醒事件分发 |
| **子节点 worker** | 任意 | 领任务、执行、上报；由 agent/脚本/工具以任意语言接入 |

- 节点**名字全网唯一**（同名重连顶掉旧连接，v1.1 语义）。
- 一台机器可运行多个节点：每个进程一个名字，端口可复用主节点监听端口（连接共享）。

## 2. 运行模式：加密 / 非加密

| 模式 | 传输 | 说明 |
|---|---|---|
| `plain` | `ws://` | 明文 WebSocket，适合可信局域网/本机 |
| `tls` | `wss://` | TLS 加密 WebSocket；主节点可自签证书（首次启动生成）或用户提供 cert/key |

- 主节点启动参数 `--mode plain|tls`（默认 `plain`）；子节点 `--mode` **必须与主节点一致**。
- `tls` 模式子节点默认信任自签证书（LAN 场景，可配置 CA 固定校验）。

### 2.1 模式不匹配 → 必须向对方报错（双保险）

**① 传输层嗅探（尽力而为，明文可读方一定能读到）**

主节点在 TLS 握手/HTTP 升级之前 peek 连接首字节：

| 服务端模式 | 客户端首字节 | 主节点动作 |
|---|---|---|
| `plain` | `0x16`（TLS ClientHello） | 回明文错误行 `STARFRUIT_ERR mode_mismatch: ...` 后断开 |
| `tls` | 非 `0x16`（明文 HTTP/WS） | 回明文错误行 `STARFRUIT_ERR mode_mismatch: ...` 后断开 |

**② 应用层声明（可靠通道，双向可读）**

v2 注册帧新增 `mode` 字段，与服务端实际模式不一致时，回执后主动 Close：

```json
{"system": true, "code": "mode_mismatch",
 "detail": "server=plain client=tls，请以 --mode plain 启动节点后重连"}
```

## 3. 注册（v2 扩展，兼容 v1.1）

连接后第一帧必须注册：

```json
{"action": "register", "from": "worker-a", "role": "worker",
 "mode": "plain", "ver": 2}
```

- `role`：`manager` / `worker`（仅展示用）；`ver`：协议版本，缺省按 1 兼容。
- 服务端回执：

```json
{"system": true, "code": "registered", "detail": "worker-a",
 "backlog": 0, "mode": "plain", "ver": 2}
```

- 同名重连顶掉旧连接（旧连接收 Close 帧）；断线重连 + 重新注册即可。
- 推荐退避：2s / 4s / 8s / 16s / 30s（上限 30s）。

## 4. 消息帧（v1.1 兼容 + v2 扩展）

```json
{
  "from": "worker-a",
  "to": "manager",          // 对端名字，或 "all" 广播（发送者不收自己的广播）
  "type": "task|chat|status|done|blocked|idle|notice|wake",
  "text": "消息正文",
  "ts": "2026-11-12 17:00:00"   // 可选
}
```

- v2 新增 `type=wake`：**唤醒消息**——接收方应触发其唤醒钩子（见 §6），用于"唤醒对话"。
- 消息类型语义沿用 ws_protocol v1.1 §4（task/status/done/blocked/idle/notice/chat）。

## 5. 回执与错误码（服务端 → 发送者，`system: true`）

| code | 含义 |
|---|---|
| `registered` | 注册成功（附 backlog / mode / ver） |
| `sent` | 定向消息已投递 |
| `queued` | 对端不在线，已暂存（v1.1） |
| `queued_delivered` | 暂存消息已投递给原发送方回执（v1.1） |
| `broadcast_ok` | 广播完成（detail 含目标数） |
| `no_such_peer` | 目标名不存在（保留兼容） |
| `bad_msg` | JSON 解析失败或字段非法 |
| `warn` | 离线队列满淘汰最旧（每对端上限 200 条） |
| `mode_mismatch` | **v2**：加密模式不匹配，服务端将断开连接 |

## 6. 唤醒钩子（agent 对接口子）

节点收到 `type=wake` 消息（或系统事件）时，执行配置的钩子之一：

| 方式 | 配置 | 行为 |
|---|---|---|
| 命令 | `--wake-cmd '<shell>'` | 以子进程执行；环境变量注入 `SF_FROM` / `SF_TO` / `SF_TEXT` / `SF_TS` |
| HTTP | `--wake-url <url>` | POST JSON `{"from","to","text","ts"}` |
| 哨兵文件 | 默认开启 | 追加一行 JSONL 到 `<data-dir>/wake.log`，agent 可 tail 该文件 |

> 钩子内容由 agent 自行对接（例如调用某个 CLI 唤醒会话）；节点本身不绑定任何 agent 实现。

## 7. Web 只读面板（主节点独有）

- 监听 `--web-port`（默认 **9502**），**无鉴权、全部只读**；TLS 模式下面板同样走 https。
- 端点：

| 端点 | 说明 |
|---|---|
| `GET /` | 可视化面板（节点拓扑 + 消息流动画 + 历史） |
| `GET /api/nodes` | 节点列表（名字、角色、模式、在线状态、收发计数、积压） |
| `GET /api/messages?limit=&before=` | 消息历史（FIFO 环形缓冲 + 可选落盘 JSONL） |
| `GET /api/stats` | 汇总统计（消息总数、在线数、运行时长、模式） |
| `GET /events` | SSE 实时事件流（message / node_join / node_leave） |

## 8. 后台运行（脱离 agent 会话）

- 所有节点支持 `--daemon`：脱离终端后台运行，写 pidfile 与日志文件；
- 提供 systemd unit 示例与 nohup 方式；进程生命周期不依赖任何 agent 会话；
- agent 唤醒对接见 §6（wake 钩子）。

## 9. 版本记录

- **v2.0（2026-11-12）**：局域网多节点拓扑；plain/tls 双模式 + 模式不匹配报错（传输层嗅探 +
  注册帧声明双保险）；注册帧新增 `role/mode/ver`；新增 `wake` 消息与唤醒钩子；
  主节点新增 Web 只读面板（HTTP + SSE）。
- **v1.1（2026-11-12）**：离线消息队列（暂存/投递/warn）、registered 附 backlog、错误处理矩阵。
  原文见 `reference/ws_protocol-v1.1.md`。
