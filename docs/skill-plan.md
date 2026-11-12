# SKILL 技能规划书 —— 多节点 WS 协同（基于 2026-11-12 经验总结）

> 来源：《2026-11-12-多Agent-WS协同开发全流程经验总结.md》§0～§6 的可复用方法论，
> 落地为一套可安装的 SKILL。协议契约见 `docs/protocol.md`（v2）。

---

## 1. 目标与定位

把"管理者 + 多执行者 + WS 消息枢纽 + 唤醒钩子"的实战模式，固化为**局域网多节点协同基建**：

- **局域网多节点**：一台机器可运行多个节点；**主节点（管理者）全网唯一**。
- **双模式协议**：加密（TLS/wss）与非加密（ws）可切换运行；**加密方式不对必须向对方报错**。
- **可视化**：主节点启动后本地监听 **Web 只读面板**（无鉴权），可见全部节点、消息流转与历史，
  消息流动带动画特效。
- **可编译交付**：主节点**全部 Rust**，安装 SKILL 后由用户**本地编译**，不提供二进制；
  子节点同功能提供 **Rust / Python / Node.js** 三种实现。
- **后台运行**：所有节点都是脱离 agent 会话的后台进程；均可**唤醒对话**（wake 钩子），
  钩子内容留给 agent 自行对接。

## 2. 功能清单

| # | 功能 | 实现方 | 说明 |
|---|---|---|---|
| F1 | 消息路由（定向/广播）+ 回执 | 主节点 | 兼容 ws_protocol v1.1 回执码 |
| F2 | 离线队列（200 条/对端 FIFO） | 主节点 | v1.1 语义：queued / queued_delivered / warn |
| F3 | plain/tls 双模式 + 不匹配报错 | 全部节点 | 传输层嗅探 + 注册帧 `mode` 声明双保险 |
| F4 | 消息历史（环形缓冲 + JSONL 落盘） | 主节点 | 面板可回看 |
| F5 | Web 只读面板（拓扑/流转/历史 + 动画） | 主节点 | HTTP + SSE，无鉴权、只读 |
| F6 | wake 唤醒钩子（命令/HTTP/哨兵文件） | 全部节点 | agent 对接口子 |
| F7 | 后台守护（--daemon / systemd / nohup） | 全部节点 | pidfile + 日志文件 |
| F8 | 三语言子节点（Rust/Python/Node.js） | 子节点 | 功能等价、协议同构 |

## 3. 组件与目录结构

```
starfruit/                        # SKILL 根目录
├── SKILL.md                      # 技能入口（安装/编译/使用说明）
├── README.md                     # 项目说明
├── CHANGELOG.md / VERSION / TODO.md
├── docs/
│   ├── 2026-11-12-多Agent-WS协同开发全流程经验总结.md   # 方法论（重整版）
│   ├── protocol.md               # starfruit 协议 v2
│   ├── skill-plan.md             # 本规划书
│   ├── naming.md                 # 命名定稿
│   └── workflow-optimized.md     # AI 编码工具工作流约束
├── master/                       # 主节点（Rust crate，用户本地编译）
│   ├── Cargo.toml
│   ├── src/                      # hub 路由 / TLS / 历史 / Web 面板 / daemon
│   └── static/                   # 面板前端（原生 HTML/JS/CSS，无构建步骤）
├── nodes/
│   ├── rust/                     # 子节点 Rust crate（用户本地编译）
│   ├── python/                   # 子节点 Python 单文件（标准库优先）
│   └── nodejs/                   # 子节点 Node.js 单文件（无第三方依赖）
└── reference/                    # 历史归档（legacy ws-hub v0.2.0 等）
```

## 4. 关键设计决策

| 决策 | 取舍 |
|---|---|
| **TLS 而非自研加密** | wss 是标准方案：Rust 用 rustls，Python 用 ssl，Node 用 tls，无自研密码学风险 |
| **模式不匹配双保险** | 传输层嗅探能立即报错但明文可能不可读（TLS 客户端遇明文）；注册帧 `mode` 声明保证错误**一定可读** |
| **面板只读 + 无鉴权** | 需求明确；绑定默认仅本机（127.0.0.1），如需局域网访问由用户显式指定 `--web-addr` |
| **HTTP + SSE 而非面板 WS** | 只读单向推送，SSE 更简单可靠，无鉴权面更小 |
| **前端无构建步骤** | 原生 HTML/JS/CSS 单目录，`include_str!` 嵌入二进制，用户零前端工具链负担 |
| **Python/Node 无第三方依赖** | 局域网离线可用；RFC6455 客户端按协议最小实现（掩码 `mask[i % 4]` 教训已记录） |
| **主节点单二进制** | hub + 面板 + TLS + daemon 一体，`cargo install --path .` 即用 |

## 5. agent 使用流程（SKILL 落地视角）

1. **安装**：拷贝 SKILL 目录 → `cargo build --release`（master + node-rust）。
2. **启动主节点**：`starfruit-master --mode plain --daemon`（或 tls + 证书）。
3. **启动子节点**：按语言选 `starfruit-node` / `starfruit_node.py` / `starfruit-node.js`，
   指定 `--name`、`--mode`（与主节点一致）、`--wake-cmd`（agent 唤醒入口）。
4. **打开面板**：浏览器访问 `http://127.0.0.1:9502`（只读）。
5. **协作循环**：按经验总结 §2.2 派发循环运转；`done/blocked/idle` 语义不变。
6. **唤醒对接**：agent 在 `--wake-cmd` 里放自己的唤醒命令（如拉起会话的 CLI），
   收到 `wake` 消息即触发。

## 6. 验收标准（对应需求逐条）

- [x] 局域网多节点、一机多节点、主节点唯一 → §3 拓扑 + `--name` 全网唯一约束
- [x] 加密/非加密可运行、模式不对向对方报错 → `docs/protocol.md` §2.1 双保险
- [x] Web 只读面板：节点、消息流转、历史、现代化界面、消息流动画 → F4 + F5
- [x] 主节点全 Rust、用户本地编译、无二进制 → `master/` crate
- [x] 子节点 Rust/Python/Node.js 功能等价 → F8
- [x] 后台进程 + 唤醒钩子留给 agent 对接 → F6 + F7
- [x] legacy ws-hub（main.rs/Cargo.toml）语义兼容保留 → `reference/legacy-ws-hub/`
