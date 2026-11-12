# 更新日志

本文件格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.5.0] - 2026-11-12
### 新增
- 主节点**启动说明**（会话提示）：打印主节点名称、运行模式、消息枢纽/面板地址、数据目录，
  以及"其它执行节点如何加入"（三语言客户端命令）与"监听地址规则"。
  子节点客户端用哪种实现**由 agent 按所在机器环境决定**（Rust / Python / Node.js 等价）。
- 文档同步补充"监听地址与暴露规则"：`docs/protocol.md` §2.2、`docs/daemon.md`、`SKILL.md`、
  `README.md`、`docs/skill-plan.md`（新增功能项 F9）。
### 变更
- 主节点默认监听地址由 `0.0.0.0:9501` 改为 `127.0.0.1:9501`（默认仅本机，最安全）。
### 安全
- **禁止监听通配地址 `0.0.0.0`（含 `::`）**：`--hub-addr` / `--web-addr` 命中通配地址时
  直接拒绝启动（退出码 1）并提示正确做法：本机 `127.0.0.1` / 局域网本机 IP /
  公网 Cloudflare Tunnel + `--mode tls`。
- 明文（`plain`）模式监听非本机地址时输出 `[warn]`，提示公网请用 Cloudflare Tunnel + `--mode tls`。
### 测试
- `tests/e2e.sh` 新增第 8 段（5 条断言）：拒绝监听 `0.0.0.0`（hub/web）与启动说明内容校验。

## [0.4.0] - 2026-11-12
### 新增
- 主节点 **starfruit-master**（纯 Rust）：WS 消息枢纽（定向/广播路由）、plain/tls 双模式（模式不匹配向对端报错）、离线队列（每对端 200 条 FIFO）、消息历史（环形缓冲 + JSONL 落盘）、`--daemon` 后台运行、wake 唤醒钩子、内置 manager 端点。
- 只读 Web 可视化面板（HTTP + SSE，无鉴权）：节点列表、消息流转拓扑（canvas 粒子动画）、实时消息流、历史筛选与分页。
- 子节点三语言实现：`nodes/rust`（starfruit-node）、`nodes/python`、`nodes/nodejs`；Python/Node 为零第三方依赖的裸 RFC6455 客户端，功能等价（注册/收发/离线队列语义/双模式接入/后台运行/wake）。
- `tests/e2e.sh` 端到端自测（7 段 22 条断言）与 `tests/mode_check.py` 双模式报错校验。
- `docs/daemon.md`：后台运行（`--daemon`/systemd/nohup）与 wake 唤醒钩子对接说明。
### 修复
- 主节点转发消息时补齐完整 JSON 帧（原仅转发纯文本，导致子节点收到非 JSON 帧）。
- 注册帧加密模式不匹配时，错误帧与 Close 按序入队冲刷后再断开，确保报错可达对端。
- Python/Node 子节点在握手阶段优先识别服务端返回的 `STARFRUIT_ERR` 明文错误行。

## [0.3.0] - 2026-11-12
### 新增
- SKILL 命名定稿：**starfruit（杨桃）**（横切面五角星对应星型拓扑），产出 `docs/naming.md` 候选对比与名称矩阵。
- `SKILL.md` 成稿：技能入口（组件一览、Quickstart、协议摘要、唤醒钩子对接、运维须知）。

## [0.2.0] - 2026-11-12
### 新增
- 《2026-11-12-多Agent-WS协同开发全流程经验总结.md》：由 MANAGEMENT.md 重新整理，去项目化、结构化重写。
- `docs/protocol.md`：starfruit 协议 v2——局域网多节点、plain/tls 双模式、模式不匹配双向报错、wake 唤醒消息。
- `docs/skill-plan.md`：SKILL 技能规划书（功能清单、目录结构、设计决策、验收标准）。
### 变更
- 原始文档归档至 `reference/`（MANAGEMENT.md、ws_protocol-v1.1.md）。

## [0.1.0] - 2026-11-12
### 新增
- 项目初始化：本地 git 仓库（main 分支）、任务台账 TODO.md。
- 上传文件归档：历史文档入 `docs/`，旧版 ws-hub 参考实现入 `reference/legacy-ws-hub/`。
