# TODO.md —— starfruit 多节点协同 SKILL 任务台账

> 追加式记录，不覆盖历史。状态仅用：待办 / 进行中 / 已完成 / 已阻塞。

### T-01 整理上传文件到规范目录
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：将用户上传的 5 个文件（main.rs、Cargo.toml、MANAGEMENT.md、ws_protocol.md、workflow-optimized.md）归档到项目规范目录
- 验收标准：①目录结构清晰（docs/ 存文档、reference/legacy-ws-hub/ 存旧版 ws-hub 参考实现）；②git 本地仓库 main 分支提交；③文件内容无损
- 涉及：docs/、reference/legacy-ws-hub/
- 方案：git init -b main；文档入 docs/，旧代码入 reference/legacy-ws-hub/
- 备注/进度：VERSION 0.1.0

### T-02 重整经验总结并规划 SKILL 技能
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：将 MANAGEMENT.md 重新整理为《2026-11-12-多Agent-WS协同开发全流程经验总结.md》；基于其内容规划一套 SKILL：局域网多节点（一机多节点、唯一主节点）、协议支持加密/非加密双模式（模式不匹配必须向对方报错）
- 验收标准：①经验总结文档去项目化、结构化重写；②SKILL.md 雏形含完整技能规划；③协议文档明确双模式与报错语义
- 涉及：docs/、SKILL.md
- 备注/进度：产出 docs/2026-11-12-多Agent-WS协同开发全流程经验总结.md（重整版，原文件归档 reference/）、docs/protocol.md（v2 双模式协议）、docs/skill-plan.md（技能规划书）。VERSION 0.2.0

### T-03 SKILL 命名定稿
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：为 SKILL 取水果/动物风格且贴合功能的名字，产出命名说明
- 验收标准：①给出候选名与理由；②定稿名与 SKILL.md、目录、各组件命名一致
- 涉及：docs/naming.md、SKILL.md
- 备注/进度：定稿 **starfruit（杨桃）**——横切面五角星 = 星型拓扑；组件命名 starfruit-master / starfruit-node 系列。SKILL.md 成稿。VERSION 0.3.0

### T-04 主节点 Rust 实现 + Web 只读可视化
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：主节点全部 Rust 实现（消息路由/离线队列/加密与非加密双模式/历史记录）；本地监听 Web 只读界面（无鉴权），展示节点、消息流转与历史，消息流动带动画特效
- 验收标准：①cargo build 通过、clippy 无警告、单测通过；②Web 界面可访问且只读；③消息流动有动画；④加密模式不匹配时报错可达对方
- 涉及：master/
- 备注/进度：已完成。cargo clippy 0 警告、5 个单测通过；面板（HTTP + SSE + canvas 粒子动画）只读可访问；双模式不匹配报错实测可达对端；e2e 覆盖全部验收项。VERSION 0.4.0

### T-05 子节点三语言实现
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：子节点提供 Rust / Python / Node.js 三种实现，功能等价（注册、收发、离线队列语义、双模式接入、后台运行）
- 验收标准：①三种实现均能接入主节点完成注册与消息收发；②Rust 版编译通过；③Python/Node 版实测跑通
- 涉及：nodes/rust/、nodes/python/、nodes/nodejs/
- 备注/进度：已完成。三语言实现均实测接入主节点并完成注册/定向/广播收发（e2e 验证）；Rust 版 clippy 0 警告、1 个单测通过；Python/Node 为零依赖裸 RFC6455 客户端

### T-06 后台守护运行与唤醒钩子
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：所有节点均为脱离 agent 会话的后台进程；提供"唤醒对话"钩子（命令/HTTP 回调）留给 agent 对接；提供 systemd/nohup 运行方式
- 验收标准：①各节点支持 daemon 方式运行；②唤醒钩子实测触发；③文档说明 agent 对接方式
- 涉及：master/、nodes/、docs/
- 备注/进度：已完成。--daemon 三端齐备（setsid/detached + pidfile + 日志）；wake 钩子（wake-cmd/wake-url/默认 wake.log 哨兵）实测触发；docs/daemon.md 含 systemd/nohup 与 agent 对接说明

### T-07 主节点启动说明与监听地址规则
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：主节点启动成功后，在会话中提示说明主节点名称、监听/面板地址，并说明其它执行节点如何加入（客户端由 agent 按机器环境决定用哪种实现）；在启动规则中明确监听地址规则：仅本机用 127.0.0.1，局域网用本机局域网 IP，**禁止 0.0.0.0**，公网建议 Cloudflare Tunnel 且必须加密模式
- 验收标准：①启动成功打印含主节点名称、加入方式、监听规则的说明；②`--hub-addr`/`--web-addr` 为 `0.0.0.0`/`::` 时被拒绝启动；③默认监听 127.0.0.1；④相关 .md 文档同步更新；⑤e2e 覆盖并通过
- 涉及：master/src/main.rs、tests/e2e.sh、SKILL.md、README.md、docs/protocol.md、docs/daemon.md、docs/skill-plan.md、CHANGELOG.md、VERSION
- 备注/进度：已完成。新增 `print_banner` 启动说明与 `validate_bind` 通配地址拦截（含 `::`），默认 hub 地址改 `127.0.0.1:9501`，明文监听非本机地址输出 `[warn]`；e2e 新增第 8 段（拒绝 0.0.0.0 + 启动说明校验），共 27 条全通过；clippy 0 警告。VERSION 0.5.0

### T-08 完善 README、架构图与流程图、开源化（MIT）文档
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：按 `docs/workflow-optimized.md` 工作流（主分支直接提交）完成开源发布准备：①README.md 补充详细使用说明、系统架构图、消息流程图；②用本地 local-llm 生成宣传图并嵌入 README；③新增 README.en.md 英文版；④项目采用 MIT 开源，补齐 LICENSE、CONTRIBUTING.md、CODE_OF_CONDUCT.md、SECURITY.md 等开源配套文档
- 验收标准：①README.md 含架构图与流程图（Mermaid，可渲染）与详细使用说明（主/子节点 CLI 参数、三种部署场景示例）；②`docs/assets/starfruit-promo.png` 实际生成并嵌入 README；③README.en.md 与中文版内容对齐；④LICENSE 为 MIT 且各文档许可声明一致；⑤CHANGELOG/VERSION 已同步；⑥e2e 与 clippy 复跑通过
- 涉及：README.md、README.en.md、LICENSE、CONTRIBUTING.md、CODE_OF_CONDUCT.md、SECURITY.md、docs/assets/、CHANGELOG.md、VERSION、master/Cargo.toml、nodes/rust/Cargo.toml
- 方案：先本地 local-llm 生成宣传图 → 改写 README（架构图/流程图/详细用法）→ 翻译英文版 → 补 MIT 开源文档 → 更新版本与日志 → 实测验证 → 提交推送
- 风险：local-llm 图片接口不转发 negative_prompt（需 guidance≤1.0 规避）；Mermaid 渲染依赖平台支持
- 备注/进度：已完成。①宣传图由本地 local-llm（Qwen-Image-2.1）生成，落盘 `docs/assets/starfruit-promo.png`（1280×736）并嵌入 README；②README.md 扩充为 9 大节（含架构图、消息流转时序图、连接与模式校验流程图 3 张 Mermaid 图、主/子节点完整参数表、三种部署场景、开发测试命令）；③新增 `README.en.md` 英文版；④新增 `LICENSE`（MIT）、`CONTRIBUTING.md`、`CODE_OF_CONDUCT.md`、`SECURITY.md`；⑤VERSION 与两个 Cargo.toml 同步升 0.6.0，CHANGELOG 记 `[0.6.0]`；⑥复跑验证：master 构建通过 + clippy 0 警告 + 5 单测通过，nodes/rust 构建通过 + clippy 0 警告 + 1 单测通过，e2e 27 通过 / 0 失败

### T-09 JWT 令牌校验 + 三种部署模式 + 端口规则
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：新增 JWT 令牌校验 + 三种部署模式（本机/局域网/公网）+ 端口规则（默认 10001、≥10001、被占用 +1）；删除 `docs/2026-11-12-多Agent-WS协同开发全流程经验总结.md`；同步更新所有受影响的 .md 文档
- 验收标准：①local/lan/public 三种部署模式校验生效；②公网（public）强制 tls + jwt；③JWT 签发/校验/借用拒绝/过期拒绝均正确；④端口规则与"被占用自动 +1"生效；⑤三语言客户端均支持 `--token` 且 `unauthorized` 时退出码为 3；⑥e2e 新增用例通过；⑦相关文档同步
- 涉及：master/、nodes/{rust,python,nodejs}/、tests/e2e.sh、SKILL.md、README.md、docs/、CHANGELOG.md、VERSION、master/Cargo.toml、nodes/rust/Cargo.toml
- 备注/进度：已完成。e2e 39 条全通过；VERSION 0.7.0

### T-10 透明 logo 图标集与 Web 面板浏览器图标
- 状态：已完成
- 创建：2026-11-12　完成：2026-11-12
- 目标：①生成透明背景的 logo 图标 `icon512.png` 及 32/64/128/256 多尺寸透明 PNG，放入 `docs/assets/`；②主节点 Web 可视化面板配置浏览器图标（favicon），并随二进制内嵌提供路由
- 验收标准：①全部 PNG 为 RGBA 全透明背景、形状为杨桃五角星；②面板 HTML 含 SVG/PNG/apple-touch-icon 图标链接；③`/static/img/*` 图标路径实际可访问且 content-type 正确；④e2e 覆盖并通过；⑤版本号与 CHANGELOG 同步
- 涉及：docs/assets/、master/static/img/、master/static/index.html、master/src/web.rs、tests/e2e.sh、CHANGELOG.md、VERSION、master/Cargo.toml、nodes/rust/Cargo.toml
- 方案：以自建 SVG（五角星 + 径向渐变 + 高光）为矢量源，用 `rsvg-convert` 渲染为各尺寸透明 PNG；面板图标经 `include_bytes!` 内嵌到 `web.rs` 并按路径返回，兼容 `/favicon.svg` 与 `/favicon.ico`
- 风险：环境无 cairosvg，改用 rsvg-convert 渲染；`/favicon.ico` 实际回 PNG（现代浏览器可识别）
- 备注/进度：已完成。docs/assets 新增 icon.svg 与 icon{512,256,128,64,32}.png、apple-touch-icon.png（180）；master/static/img 新增 logo.svg 与 favicon-{32,64,128,256}.png、apple-touch-icon.png；实测 PNG 为 srgba 透明（角落 alpha=0，非透明像素占比 28.66%），形状为五角星；面板 7 条图标路径实测均 200 且 content-type 正确；e2e 新增 3 条断言，共 42 通过 / 0 失败；VERSION 0.8.0
