# 更新日志

本文件格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.11.0] - 2026-11-20
### 新增
- **Tunnel 脚本支持 TLS 主节点**：`scripts/starfruit-tunnel.sh` 原先写死 `http://` 转发，仅适配 plain；
  public 模式强制 tls 时会 mode_mismatch 断连。现新增模式参数 `plain|tls|auto`（默认 auto，
  或 `STARFRUIT_TUNNEL_MODE`）：tls 用 `https://` + `--no-tls-verify`（自签证书）；auto 先探测主节点
  实际模式（TLS ClientHello 嗅探）、再读 `~/.starfruit/master/setup.json`、回退 plain。
  实测 plain/tls 主节点探测均正确；文档（SKILL/daemon）同步。（原记录于 Unreleased，随本版归档）
### 修复
- **Tunnel 看门狗启动即崩（unbound variable）**：UTF-8 locale 下脚本中 `$mode`/`$code` 后紧跟全角括号
  时，bash 将全角字符首字节并入变量名，`set -u` 下报 `mode: unbound variable`，看门狗启动即崩、
  隧道无法建立（含无效模式报错路径与看门狗重启路径共 4 处）。现全部改为 `${mode}`/`${code}`
  花括号引用。隔离环境实测：原始脚本复现崩溃；修复后无效模式报错正常、公网 curl 回环可达、
  cloudflared 退出后看门狗自动重启并取得新地址。

## [0.10.0] - 2026-11-18
### 新增
- **用户级数据目录**：所有操作系统统一 `~/.starfruit`（HOME/USERPROFILE）。主节点默认
  `~/.starfruit/master`，子节点默认 `~/.starfruit/nodes/<name>`（`--data-dir` 可覆盖）。
- **首次启动向导**：首启（无 `setup.json` 且 stdin 为终端）或 `--setup` 进入交互向导：
  部署模式 → 加密模式 → JWT 与令牌有效期（24h/7d/30d/365d/自定义秒）→ 工作团队与成员 →
  节点文档路径。选择持久化 `setup.json`，之后启动自动复用（`--no-setup` 跳过）。
- **节点接入文档 `starfruit-nodes.md`**：首启生成、节点信息变动时自动更新（上线/下线/令牌/Tunnel）。
  含主节点地址端口、面板、公网 Tunnel 地址、团队成员名/角色/在线/令牌、每成员连接提示词
  （```text 代码块）与其它节点连接命令。路径决策：显式 > `docs/` > `doc/` > 根目录；自动加入 `.gitignore`。
- **令牌集中管理**：`<data-dir>/tokens.json`（0600）；团队每次启动重新签发；`--issue-token` 合并写入；
  启动横幅向用户展示成员名 + 令牌。
- **节点名称全网唯一**：创建（团队成员去重、不得与管理者同名）与登录（在线重名拒绝）双重校验；
  服务端回执 `name_conflict`，三语言客户端退出码 **4**（不顶替旧连接）。
- **独立 Cloudflare Tunnel**：`scripts/starfruit-tunnel.sh start|stop|restart|status|url`，
  数据 `~/.starfruit/tunnel`（pid/日志/url）。看门狗崩溃自动拉起；**主节点重启不影响公网地址**，
  仅显式 stop/restart 才变更；停止只按本脚本记录的精确 PID 操作。主节点读取 `~/.starfruit/tunnel/url` 展示。
- **被动唤醒自动对接**：`scripts/starfruit-wake-setup.sh <wake.log> [唤醒命令]` 检测本机 agent
  （Claude Code 合并 `hooks.SessionStart`，幂等）并后台常驻唤醒桥；其它 agent 打印接入说明。
- **cucumber 可选集成**：`scripts/starfruit-cucumber.sh list|add|start|stop`，自动读取配置
  （macOS `~/Library/Application Support/cucumber/config.toml` / 其它 `~/.config/cucumber/config.toml`）；
  `add` 须用户同意（`--yes`）；绝不 DELETE 进程配置；不装 cucumber 不影响任何功能。
- **每周更新检查**：主节点启动与三语言子节点每次连接比对线上 CHANGELOG 最新版本
  （仓库固定 `https://github.com/chunshilaioqf/starfruit-skill`），节流 7 天（`update-check.json`，
  统一 `ts_ms/latest` schema）；发现新版本在会话提醒 + 面板横幅。`scripts/starfruit-update.sh`
  拉取 + 重编译 + 数据迁移检查提示（不触碰运行中进程与 Tunnel）。
- **数据迁移检查**：`<data-dir>/data-version` 标记，启动时对比版本（当前各版本 JSONL 结构兼容）。
### 变更
- **Web 面板全面改版**：中英双语（默认中文，顶栏切换，localStorage 持久化）；消息历史改为
  弹窗（80vw×80vh，min 640×320，按钮位于实时连接 chip 之后）；拓扑动画升级——工作节点旋转齿轮、
  空闲呼吸灯、消息放大高亮 5s 缓慢回落；HUB 改用 SVG logo（新消息快转 5s / 新节点上线闪烁 10s /
  节点离线红色剧烈跳动 10s）；节点显示收/发计数、点击弹出实时流最近 5 条（无则「暂无」）；
  标题右端单色/多彩切换（多彩=名称哈希 HSL）；节点居中旋转首字符（背景反色+75% 白+白辉光）；
  拓扑 20% 灰网格背景；页脚 GitHub 链接 + 版本号；更新横幅。
- 子节点默认数据目录改为 `~/.starfruit/nodes/<name>`；`--help` 与团队提示词同步更新。
### 测试
- `tests/e2e.sh` 新增第 13–16 段（名称唯一退出码 4、首启向导/节点文档生成与动态更新/二次启动复用、
  用户级默认目录、tunnel/wake-setup/cucumber/update 脚本行为），共 **93 条**全通过；
  `master` 单测 26 条、Rust 子节点单测 2 条全通过；真实 Chrome 无头浏览器实测面板
  （中英切换/历史弹窗/单色多彩/节点弹窗/更新横幅显隐/拓扑动画）均正常。

## [0.9.0] - 2026-11-15
### 新增
- **后台运行 + 保活**：明确要求所有节点（含主节点）**以脱离 agent 会话的方式后台运行**并**自建保活**；
  新增 `scripts/starfruit-keepalive.sh`（watchdog：异常退出按 2s→30s 指数退避重启，正常退出不重启）。
- **hooks 被动唤醒桥**：新增 `scripts/starfruit-wake-bridge.sh`——复用 agent 的 hooks 技术，
  `tail -F <data-dir>/wake.log` 被动接收主节点任务，经 `STARFRUIT_WAKE_CMD` 唤醒当前会话继续工作
  （`python3` + `shlex.quote` 安全注入 `SF_FROM/SF_TO/SF_TEXT/SF_TS`，零第三方依赖）。
- **工作团队（team）**：主节点 `--team <team.json>` 为每位成员生成「加入团队」完整提示词；
  `--init-team <path>` 写出配置模板。提示词经三处「代码区」交付：会话输出（`text` 代码块）、
  落盘 `<data-dir>/team-prompts.md`、面板 `/team` 页面（`<pre><code>` + 一键复制）。
- 主节点未配置团队时，启动后**提醒用户是否需要配置工作团队**。
- 只读面板新增端点：`GET /team`（页面）、`GET /api/team`（JSON）、`GET /team-prompts.md`（Markdown）。
- `master/src/team.rs`：团队定义解析（team/members）与提示词生成（含 4 个单测）。
- `scripts/starfruit-team.example.json`：团队配置示例（与 `--init-team` 模板一致）。
### 变更
- 主节点启动流程重排：先绑定 hub（拿到真实地址）→ 加载团队 → 绑定面板 → 打印 banner →
  输出/落盘团队提示词（或给出未配置提醒）。
- 文档同步：`SKILL.md`、`README.md`、`README.en.md`、`docs/daemon.md`、`docs/protocol.md`
  补「后台运行+保活」「hooks 被动唤醒桥」「工作团队」三项能力。
### 测试
- `tests/e2e.sh` 新增第 12 段（14 条断言：`--init-team`/`--team`/`/api/team`/`/team`/
  `/team-prompts.md`/未配置提醒，以及保活与唤醒桥脚本行为），共 **56 条**全通过；
  `master` 单测 14 条全通过（含 4 个 team 单测）。

## [0.8.0] - 2026-11-12
### 新增
- 透明 **logo 图标集**：`docs/assets/icon.svg`（矢量源）与 `icon512/256/128/64/32.png`、
  `apple-touch-icon.png`（180）；图形为杨桃横切面五角星，RGBA 全透明背景。
- 只读 **Web 面板浏览器图标**：`/static/img/logo.svg`、`favicon-32/64/128/256.png`、
  `apple-touch-icon.png`（随二进制内嵌），并兼容 `/favicon.ico`。
### 测试
- `tests/e2e.sh` 新增 3 条断言（首页含图标链接、favicon PNG 与 logo SVG 可访问），共 42 条全通过。

## [0.7.0] - 2026-11-12
### 新增
- **JWT 令牌校验**：主节点 `--issue-token <节点名>` 签发（签发后打印并退出），子节点 `--token <令牌>`
  连接校验；Rust / Python / Node.js 三语言客户端均支持。
- **三种部署模式** `--deploy local|lan|public`（默认 `local`）及校验：`local` 只监听回环
  `127.0.0.1`；`lan` 监听本机局域网 IP、拒绝回环外的通配地址；`public` 禁止 `0.0.0.0` 且必须指定
  具体 IP、强制 `--mode tls` + JWT。
- 主节点参数：`--deploy`、`--jwt-secret`、`--jwt-ttl`、`--require-jwt`、`--issue-token`。
- `master/src/auth.rs`：JWT 签发与校验（HS256，基于 `ring` + `base64`）。
- `tests/e2e.sh` 新增第 9 / 10 / 11 段用例（部署模式、JWT、端口规则）。
### 变更
- 默认端口由 `9501` / `9502` 改为 **`10001`**（hub）/ **`10002`**（面板）；所有模式端口必须 ≥10001，
  不满足或被占用时自动 +1。
- 子节点默认 `--server` 改为 **`ws://127.0.0.1:10001`**。
### 安全
- **公网（`public`）模式强制 tls + jwt**。
- **令牌不可跨节点借用**（`sub` 与连接节点名绑定），过期即拒绝。
- JWT 密钥文件 `data-dir/jwt-secret` 以权限 `0600` 写入。
### 测试
- `tests/e2e.sh` 断言扩至 **39 条**，全部通过。
### 其他
- 删除 `docs/2026-11-12-多Agent-WS协同开发全流程经验总结.md`，同步更新受影响的 .md 文档。

## [0.6.0] - 2026-11-12
### 新增
- `README.md` 大幅扩充：核心特性表、**系统架构图**、**消息流转时序图**、**连接与模式校验流程图**
  （均为 Mermaid）、主/子节点完整参数表、三种部署场景（本机 / 局域网 / 公网 Cloudflare Tunnel）示例、
  开发与测试命令；并嵌入宣传图 `docs/assets/starfruit-promo.png`。
- `README.en.md`：README 英文版（内容与中文版对齐）。
- 开源配套文档：`LICENSE`（MIT）、`CONTRIBUTING.md`（贡献指南）、`CODE_OF_CONDUCT.md`（行为准则）、
  `SECURITY.md`（安全策略与漏洞上报）。
### 变更
- 项目正式采用 **MIT 许可证**，README（中/英）新增许可与贡献入口说明。

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
