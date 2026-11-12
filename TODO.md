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
- 状态：待办
- 创建：2026-11-12　完成：-
- 目标：为 SKILL 取水果/动物风格且贴合功能的名字，产出命名说明
- 验收标准：①给出候选名与理由；②定稿名与 SKILL.md、目录、各组件命名一致
- 涉及：docs/naming.md、SKILL.md
- 备注/进度：-

### T-04 主节点 Rust 实现 + Web 只读可视化
- 状态：待办
- 创建：2026-11-12　完成：-
- 目标：主节点全部 Rust 实现（消息路由/离线队列/加密与非加密双模式/历史记录）；本地监听 Web 只读界面（无鉴权），展示节点、消息流转与历史，消息流动带动画特效
- 验收标准：①cargo build 通过、clippy 无警告、单测通过；②Web 界面可访问且只读；③消息流动有动画；④加密模式不匹配时报错可达对方
- 涉及：master/
- 备注/进度：-

### T-05 子节点三语言实现
- 状态：待办
- 创建：2026-11-12　完成：-
- 目标：子节点提供 Rust / Python / Node.js 三种实现，功能等价（注册、收发、离线队列语义、双模式接入、后台运行）
- 验收标准：①三种实现均能接入主节点完成注册与消息收发；②Rust 版编译通过；③Python/Node 版实测跑通
- 涉及：nodes/rust/、nodes/python/、nodes/nodejs/
- 备注/进度：-

### T-06 后台守护运行与唤醒钩子
- 状态：待办
- 创建：2026-11-12　完成：-
- 目标：所有节点均为脱离 agent 会话的后台进程；提供"唤醒对话"钩子（命令/HTTP 回调）留给 agent 对接；提供 systemd/nohup 运行方式
- 验收标准：①各节点支持 daemon 方式运行；②唤醒钩子实测触发；③文档说明 agent 对接方式
- 涉及：master/、nodes/、docs/
- 备注/进度：-
