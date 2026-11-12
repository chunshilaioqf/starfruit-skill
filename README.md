# starfruit（杨桃）—— 多节点 WS 协同 SKILL

⭐ 星型拓扑消息枢纽：**唯一主节点（Rust）+ 多种子节点（Rust / Python / Node.js）**。
局域网多节点协同（一机可跑多节点）、plain/tls 双加密模式（模式不匹配向对方报错）、
只读 Web 可视化面板（消息流动画）、离线队列、消息历史、wake 唤醒钩子。

- **安装即用**：Rust 组件由用户本地 `cargo build`（不提供二进制）；
- **后台运行**：所有节点为脱离 agent 会话的后台进程，wake 钩子留给 agent 对接。

## 快速开始

详见 [SKILL.md](SKILL.md)（技能入口）与 [docs/daemon.md](docs/daemon.md)（守护/唤醒）。

```bash
cd master && cargo build --release          # 编译主节点
./target/release/starfruit-master --mode plain --daemon
open http://127.0.0.1:9502                  # 只读面板
```

> 启动成功后会话中会打印**启动说明**（主节点名称、监听地址、子节点加入方式、监听规则）。
> 监听地址：**仅本机用 `127.0.0.1`（默认）**；局域网用**本机局域网 IP**；**禁止 `0.0.0.0`**；
> 公网建议用 **Cloudflare Tunnel** 并**必须 `--mode tls`**。详见 [SKILL.md](SKILL.md#2-启动主节点唯一管理者)。

## 文档地图

| 文档 | 内容 |
|---|---|
| [SKILL.md](SKILL.md) | 技能入口：组件、Quickstart、协议摘要、运维须知 |
| [docs/protocol.md](docs/protocol.md) | starfruit 协议 v2（拓扑/双模式/报错语义/回执码/wake） |
| [docs/2026-11-12-多Agent-WS协同开发全流程经验总结.md](docs/2026-11-12-多Agent-WS协同开发全流程经验总结.md) | 方法论来源（管理者/执行者/巡检/验收/陷阱） |
| [docs/skill-plan.md](docs/skill-plan.md) | SKILL 规划书（功能清单/设计决策/验收标准） |
| [docs/naming.md](docs/naming.md) | 命名说明（starfruit = 杨桃横切面五角星 = 星型拓扑） |
| [docs/daemon.md](docs/daemon.md) | 后台运行（--daemon/systemd/nohup）与 wake 唤醒钩子 |
| [docs/workflow-optimized.md](docs/workflow-optimized.md) | AI 编码工具工作流约束 |

## 目录结构

```
starfruit/
├── SKILL.md / README.md / CHANGELOG.md / VERSION / TODO.md
├── docs/            # 协议、方法论、规划、命名、守护、工作流
├── master/          # 主节点（Rust）：hub + TLS + 历史 + Web 面板 + daemon
├── nodes/
│   ├── rust/        # 子节点 Rust（cargo build）
│   ├── python/      # 子节点 Python（零第三方依赖）
│   └── nodejs/      # 子节点 Node.js（零第三方依赖，Node ≥ 18）
└── reference/       # 历史归档（legacy ws-hub v0.2.0、原协议/经验文档）
```
