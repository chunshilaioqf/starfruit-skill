# 贡献指南（Contributing）

感谢你对 **starfruit（杨桃）** 的关注！本文说明如何参与开发、如何提交改动与如何验证。

## 行为准则

参与本项目即表示你认同并遵守 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。

## 许可证

本项目采用 [MIT 许可证](LICENSE)。你提交的任何贡献（Issue、Pull Request、补丁、文档等）
均视为在该许可证下授权给本项目。

## 环境要求

| 组件 | 要求 |
|---|---|
| 主节点 / Rust 子节点 | Rust 1.75+（`cargo`、`cargo clippy`） |
| Python 子节点 | Python 3.8+（**仅标准库**，无第三方依赖） |
| Node.js 子节点 | Node.js ≥ 18（**仅内置模块**，无第三方依赖） |
| 端到端测试 | Bash（macOS / Linux） |

## 开发流程

1. **先读文档**：`README.md`、`SKILL.md`、`docs/protocol.md`、`docs/workflow-optimized.md`
   与 `TODO.md`（任务台账，采用**追加式**记录，不覆盖历史）。
2. **开新分支**：`feat/<简述>`、`fix/<简述>`、`docs/<简述>` 等。
3. **一次只做一件事**：改动尽量小而聚焦，便于评审与回滚。
4. **写测试**：新增或变更行为需补测试；缺陷修复应能复现该缺陷（测试或明确的复现步骤）。
5. **实际运行验证**（见下节），如实报告结果；未验证的内容须明确标注。
6. **提交**：使用简体中文的 Conventional Commits 信息，并在正文关联 `TODO.md` 编号。
7. **推送并发起 PR**：说明改动原因、验证方式与影响范围。

## 验证命令

```bash
# 主节点：构建 / 静态检查 / 单元测试
cd master      && cargo build --release && cargo clippy -- -D warnings && cargo test

# 子节点（Rust）
cd nodes/rust  && cargo build --release && cargo clippy -- -D warnings && cargo test

# 端到端测试（主节点 + 三语言子节点：注册、定向/广播、离线队列、监听规则等）
bash tests/e2e.sh
```

> 禁止为了让检查“通过”而弱化/删除断言、写特判、吞异常或关闭检查。请修复根因。

## 代码与文档规范

- 交流、文档、代码注释、提交信息、`CHANGELOG.md` 使用**简体中文**；
  代码标识符、配置键、命令、报错原文保持**英文/原文**。
- 代码风格遵循各语言官方推荐规范，结构清晰、模块化，注释解释“为什么”而非“是什么”。
- **不引入无必要的新依赖**：子节点必须保持零第三方依赖（仅标准库/内置模块）。
- 协议（`docs/protocol.md`）字段变更属于**契约变更**，须评估兼容性并在 PR 中显著说明。

## 安全红线

- **禁止**在代码、配置样例、日志、文档、提交信息中提交任何凭据（token、密码、私钥、API Key）。
- 提交前自查：`git status` 与 `git diff --staged`，确认无凭据、无调试代码、无构建产物。
- 安全漏洞请**不要**通过公开 Issue 披露，按 [SECURITY.md](SECURITY.md) 的方式上报。
- 涉及监听地址的改动必须遵守“**禁止监听 `0.0.0.0`**”规则（见 README 与 `docs/protocol.md`）。

## 提交信息示例

```
feat(node): 子节点支持 --ca-cert 固定校验自签证书

- 为 TLS 模式补充 CA 固定校验能力，避免依赖“信任一切”
- 关联 TODO：T-0xx
```

```
fix(master): 修复离线队列满载时未回执 warn 的问题

- 关联 TODO：T-0xx
```

## 分支与发布

- `main` 为稳定分支；发布时打 tag（如 `v0.6.0`）并归档 `CHANGELOG.md` 的 `Unreleased` 条目。
- 版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)，
  且 `VERSION`、各 `Cargo.toml`、`CHANGELOG.md` 必须保持一致。
