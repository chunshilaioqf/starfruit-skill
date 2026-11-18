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

### T-11 后台保活 + hooks 被动唤醒 + 工作团队（team）
- 状态：已完成
- 创建：2026-11-15　完成：2026-11-15
- 目标：①告知各 agent 节点启动须**脱离会话后台运行**并**自建保活机制**；②**复用 agent 的 hooks 技术被动接收主节点任务、唤醒当前 agent 继续工作**；③主节点启动完成后**提醒是否配置工作团队**，已配置则为每位成员生成「加入团队」完整提示词，放在**代码区**方便复制。
- 验收标准：①`--daemon` 语义与保活脚本 `scripts/starfruit-keepalive.sh` 可用且文档化；②提供 hooks 唤醒桥 `scripts/starfruit-wake-bridge.sh`（`tail -F wake.log` → 唤醒会话）；③`--init-team`/`--team` 生成每位成员提示词，输出到会话 / `team-prompts.md` / 面板 `/team`（含一键复制）；④未配置团队时启动提醒；⑤e2e 与单测复跑全通过；⑥版本与 CHANGELOG 同步。
- 涉及：master/src/team.rs、master/src/main.rs、master/src/web.rs、scripts/、SKILL.md、README.md、README.en.md、docs/daemon.md、docs/protocol.md、tests/e2e.sh、CHANGELOG.md、VERSION、master/Cargo.toml、nodes/rust/Cargo.toml
- 方案：新增 `team.rs`（解析 + 提示词生成 + 视图）与 `--team`/`--init-team`；重排 `run()` 先绑定 hub 再生成提示词；面板新增 `/team`、`/api/team`、`/team-prompts.md`；新增两个 shell 脚本；文档三处同步提示词与保活/唤醒用法。
- 风险：提示词内含 `--`、`|` 等字符，e2e 断言需 `grep -q --` 规避选项解析；markdown 内 ```text 反引号在 bash 双引号下会触发命令替换（改用成员名断言规避）。
- 备注/进度：已完成。master clippy 0 警告、14 单测通过；e2e 新增第 12 段 14 条断言，共 56 通过 / 0 失败；VERSION 0.9.0。

### T-12 用户级目录 + 首启向导/节点文档 + 名称唯一 + 独立 Tunnel + 唤醒自动对接 + cucumber + 更新检查 + 面板改版
- 状态：已完成
- 创建：2026-11-18　完成：2026-11-18
- 目标：①节点安装/启动后自动与当前 agent 工具被动唤醒对接；②cucumber 存在时可选注册节点进程（用户决定）；③公网 Cloudflare Tunnel 独立后台运行、主节点重启不换地址、仅显式 stop/restart 才操作；④首启交互向导（部署→团队/令牌有效期→初始化启动）并在会话告知地址/成员/令牌，生成 starfruit-nodes.md（用户路径>docs>根）加 .gitignore、含各节点连接提示词代码块；⑤节点信息变动时文档动态更新；⑥面板拓扑动画（工作齿轮/空闲呼吸/消息放大 5s 回落；HUB 用 SVG logo：新消息快转 5s/上线闪烁 10s/离线红跳 10s）+ 中英切换（默认中文）+ 历史弹窗 80vw×80vh(min640×320)；⑦拓扑节点收/发状态、点击弹最近 5 条（无则暂无）、标题右端单色/多彩切换、居中旋转首字符（反色+75%白+白辉光）、20% 灰网格；⑧节点名创建+登录双重唯一；⑨每周更新检查（节点连接+主节点启动）提醒+数据迁移检查；⑩数据统一 ~/.starfruit 跨 OS；⑪仓库默认 github.com/chunshilaioqf/starfruit-skill、更新比对线上 CHANGELOG、面板含 GitHub 链接。
- 验收标准：①wake-setup 幂等注册 agent hook 并常驻唤醒桥；②cucumber 脚本只读 list 可用、add 须同意、不 DELETE；③tunnel 脚本 start/stop/restart/status/url 精确 PID、主节点读 url；④向导落盘 setup.json/tokens.json(0600)/starfruit-nodes.md+.gitignore、二次启动复用；⑤文档随上线/下线动态更新；⑥⑦面板真实浏览器实测中英/弹窗/配色/节点弹窗/动画/横幅；⑧name_conflict 退出码 4 不顶替；⑨update-check 节流+迁移标记；⑩默认目录 ~/.starfruit/nodes/<name>；⑪stats 含 repo_url/version、页脚 GitHub。
- 涉及：master/src/{paths,update,setup,nodesdoc,team,hub,protocol,main,web}.rs、master/static/{index.html,app.js,style.css}、nodes/{rust/src/main.rs,python/starfruit_node.py,nodejs/starfruit-node.js}、scripts/{starfruit-tunnel,starfruit-wake-setup,starfruit-cucumber,starfruit-update}.sh、tests/e2e.sh、SKILL.md、README.md、README.en.md、docs/{protocol,naming,daemon,skill-plan}.md、CHANGELOG.md、VERSION、master/Cargo.toml、nodes/rust/Cargo.toml、.gitignore
- 方案：新增 paths/update/setup/nodesdoc 四个 master 模块；hub 注册改拒绝在线重名；三客户端默认目录+name_conflict 退出 4+每周更新检查；四个运维脚本；面板前端重写（i18n/历史弹窗/拓扑动画/配色/节点弹窗/更新横幅）。
- 风险：①Moli 自研浏览器 canvas2d 不完整致残影，改用真实 Chrome headless(CDP 9223) 实测；②drawNode 时间参数 t 遮蔽 i18n 函数 t 致崩溃，改名 sec；③master 与节点 update-check schema 不一致，统一 ts_ms/latest；④update-banner display:flex 覆盖 hidden，补 [hidden]{display:none}；⑤performance.now() 从 0 起致启动瞬间误判工作中，activity/flash 缺省改 -1e9。
- 备注/进度：已完成。master clippy 0 警告、26 单测通过；Rust 子节点 2 单测通过；e2e 新增 13–16 段共 93 通过 / 0 失败；真实 Chrome 实测面板中英/历史弹窗/单色多彩/节点弹窗/更新横幅显隐/拓扑齿轮与呼吸动画均正常；开发全程避开生产（10001/10002、coder、cloudflared），测试用 195xx/197xx 端口与隔离 HOME，清理仅按精确 PID。VERSION 0.10.0。
