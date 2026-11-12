# ws_protocol.md —— ZCode ↔ Trae 本机直连通讯协议 v1.1

> 本文档是双方共同遵守的通讯契约。ws-hub 由 ZCode 负责运行与维护（常驻进程）。
> 版本：**v1.1**（2026-11-12），变更记录见文末第 9 节。

## 1. 基本参数

| 项 | 值 |
|---|---|
| 地址 | `ws://127.0.0.1:9501` |
| 鉴权 | **无**（仅绑定本机回环，外部不可达） |
| 协议 | WebSocket 文本帧，载荷为 UTF-8 JSON |
| 服务端 | `tools/ws-hub/`（Rust + tokio-tungstenite，ZCode 编写并负责运行） |

## 2. 角色与注册

| 名字 | 角色 | 职责 |
|---|---|---|
| `zcode` | **总管** | 任务分派、验收、进度把关；管理 ws-hub 进程 |
| `trae` | **执行者** | 领取任务、持续实现（data-lake 等）、完成后主动上报 |

**连接后第一帧必须注册**：

```json
{"action": "register", "from": "trae"}
```

- 服务端回执：`{"system": true, "code": "registered", "detail": "trae", "backlog": 0}`
  - v1.1 起回执带 `backlog` 字段 = 注册时该名字的**离线积压条数**；注册成功后 hub 自动按 FIFO
    逐条投递积压消息，并向每条的原发送方回 `queued_delivered`（见第 3 节）
- 同名重连会顶掉旧连接（旧连接收到 Close 帧）；断线后直接重连 + 重新注册即可
- 推荐重连退避：2s / 4s / 8s / 16s / 30s（上限 30s，恢复后立即回到工作循环）

## 3. 消息格式（发送帧）

```json
{
  "from": "trae",
  "to": "zcode",          // 对端名字，或 "all" 广播（发送者不收自己的广播）
  "type": "task|chat|status|idle|notice",
  "text": "消息正文（纯文本或序列化的 JSON 字符串均可）",
  "ts": "2026-11-12 04:05:00"   // 可选，墙钟时间
}
```

**服务端回执**（发给发送者，`system: true`）：

| code | 含义 |
|---|---|
| `sent` | 已投递到目标 |
| `broadcast_ok` | 广播完成，detail 含目标数 |
| `no_such_peer` | 目标不在线（detail 列出当前在线名单）|
| `bad_msg` | JSON 解析失败或字段非法 |
| `registered` | 注册成功（v1.1 起附 `backlog` 字段 = 待投递积压数）|
| `queued` | **v1.1**：目标不在线，消息已暂存（detail 含当前积压数），发送方知晓非即时投递 |
| `queued_delivered` | **v1.1**：暂存消息已在目标注册后投递，逐条发给**原发送方**（detail 含 `to` 与 `seq=i/n`）|
| `warn` | **v1.1**：目标离线队列已满（每对端上限 200 条），最旧一条被淘汰（发给发送方）|

**v1.1 语义变化说明**：对有效目标名的**定向消息**，对端不在线时由回 `no_such_peer`
改为**暂存并回 `queued`**（`no_such_peer` 代码保留兼容，正常路由中不再触发）；
**广播语义不变**（仍只投递当前在线者，离线者不补发）。

转发保持消息 JSON 原样；`system` 帧不是转发消息，不携带 `from/to`。

## 4. 消息类型约定（双方遵守）

| type | 方向 | 语义 |
|---|---|---|
| `task` | zcode → trae | 派发任务；text 内含任务编号（如 `T-011`）、目标文件、验收标准 |
| `status` | trae → zcode | 进度心跳：任务进行中的关键节点（开始实现某文件、编译通过等），**每完成一个文件或每 10 分钟至少一条** |
| `done` | trae → zcode | 任务完成上报；text 含：改动文件清单、验证命令与结果（build/clippy/test 实际输出摘要） |
| `blocked` | trae → zcode | 遇到阻塞（契约冲突、编译失败 3 次、需要决策）；text 说明现状与已尝试方案 |
| `idle` | trae → zcode | 当前无任务，请求派发；zcode 收到后立即回 `task` |
| `notice` | 任意 → 任意/all | 通用通知 |
| `chat` | 任意 | 自由文本 |

**工作循环（Trae 侧）**：注册 → 若手头无任务则发 `idle` → 收 `task` 后开工 → 每节点发 `status` → 完成发 `done` → 继续发 `idle` 领下一个任务。**阻塞时发 `blocked` 等待总管决策，不要自行扩大范围或停摆**。

**总管义务（ZCode 侧）**：收到 `idle` 立即派发下一任务（参照 TODO.md 任务归属）；收到 `done` 验收（build/clippy/test + API 实测）后回 `notice`（通过/打回）；收到 `blocked` 给出决策或升级到用户。

## 5. 客户端连接示例

### Python（Trae 侧可直接使用）

```python
import asyncio, json
try:
    import websockets as _ws  # pip install websockets
    async def main():
        async with _ws.connect("ws://127.0.0.1:9501") as ws:
            await ws.send(json.dumps({"action": "register", "from": "trae"}))
            print(json.loads(await ws.recv()))          # registered 回执
            await ws.send(json.dumps({"from": "trae", "to": "zcode",
                                      "type": "idle", "text": "请求任务"}))
            while True:
                print("收到:", json.loads(await ws.recv()))
    asyncio.run(main())
except ImportError:
    ...
```

无第三方库时用标准库裸实现（握手 + RFC6455 掩码帧，参照 `/tmp/ws_hub_test.py` 的
`connect_and_register/send/recv_text` 三函数，**注意掩码必须循环 `mask[i % 4]`**）。

### Rust（项目内）

```rust
// tokio-tungstenite 已在 workspace 缓存
let (ws, _) = tokio_tungstenite::connect_async("ws://127.0.0.1:9501").await?;
// 之后 sink.send(Message::text(json)) / stream.next()
// 可运行示例：cargo run --manifest-path tools/ws-hub/Cargo.toml --example probe -- trae
```

## 6. 进程管理（ZCode 侧义务）

- ws-hub 由 ZCode 以后台任务常驻运行；进程退出会通知 ZCode，ZCode 负责重启
- 重启命令：`cargo build --manifest-path tools/ws-hub/Cargo.toml && ./tools/ws-hub/target/debug/ws-hub`
- 可用 `WS_HUB_ADDR` 环境变量改绑定地址（默认 `127.0.0.1:9501`）
- 服务端日志打印在启动它的终端流：注册/离线/路由/广播全程可审计

## 7. 验收记录

- 2026-11-12 ZCode 实测 9/9 通过：注册、定向互发、回执、广播（发送者不收）、
  no_such_peer、同名重连顶掉旧连接、新连接路由、对端断开清理后 no_such_peer
- 服务端 clippy `-D warnings` 通过

## 8. 错误处理与相互通知（v1.1 增补，双方遵守）

**总原则：发生错误时必须通知对方（WS 在线用 WS，断线用留言板），并按错误类型执行对应策略，
不允许静默吞错或无通知地长时间停摆。**

| 错误类型 | 处理策略 | 通知动作 |
|---|---|---|
| 请求超限/限流（HTTP 429、hub/接口限流） | **等待**：按退避（2s→30s）挂起请求，恢复后继续；**不得**立即重试轰炸 | `status` 帧注明"限流等待，预计 Xs"；恢复后再发一条 |
| 服务不可用（连接拒绝/超时/502 已重试穷尽） | 按 WS/REST 既有退避重试；连续 3 轮失败转 `blocked` | `status`/`blocked` 帧给对方与用户 |
| 认证失效（401） | 停止重试，提示**用户**重新生成 token（凭据只经用户手） | `blocked` 帧 + 留言板记录 |
| 编译/测试连续失败 3 次 | 停止盲试，转 `blocked` 附已尝试方案清单 | `blocked` 帧 + 留言板 |
| **配额/套餐耗尽**（自身模型配额、套餐到期） | 立即停止派发/执行，等待容量恢复或套餐切换 | `blocked` 帧注明类型与预计恢复时间；**对方收到后暂停派任务，等恢复通知** |
| 对端长时间无响应（>15 分钟无 status） | 请求方视为对端已停摆：先 `notice` 询问，再留言板确认，仍无回应则把其任务标记挂起 | 留言板记录挂起决定 |
| 数据错误（契约违反/语义 bug） | `blocked` 附最小复现证据；协调者（zcode）裁决归属与修复方 | `blocked` 帧 + 留言板 |

**套餐/容量切换约定（用户指令，2026-11-12 记录）**：
- zcode 当前运行于 **smart plan（错峰空闲套餐），有效期至 2026-11-13 09:00**；
- **到期后 zcode 侧切换为用户的个人编码套餐**（模型/套餐的最终生效由用户在客户端配置，
  agent 侧义务是：感知到配额/套餐类错误时，立即按上表通知并挂起，恢复后主动发 `notice` 续跑）；
- trae 侧同理：遇到配额类错误即 `blocked` 通知 zcode，zcode 相应调整派发节奏。

## 9. 版本记录

- **v1.1（2026-11-12）**：
  - **ws-hub 离线消息队列**（hub 实现 `tools/ws-hub` v0.2.0，已上线验收版）：对端不在线时按
    目标名暂存消息（每对端 FIFO、上限 200 条，超限淘汰最旧并回 `warn`）；对端注册成功后自动
    按 FIFO 投递积压，并逐条向原发送方回 `queued_delivered`；发送方在暂存时收到 `queued` 回执
  - `registered` 回执新增 `backlog` 字段（注册时待投递积压数，可为 0）
  - **语义变化**：离线定向消息回 `queued`（原为 `no_such_peer`）；广播语义不变
  - §8「错误处理与相互通知」增补（限流等待 / 配额挂起 / 15 分钟无响应挂起等处置矩阵）
