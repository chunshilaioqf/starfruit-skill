//! 工作团队（team）：成员清单解析 + 每位成员"加入团队"提示词生成 + 面板视图。
//!
//! 主节点启动时可加载一份团队定义（`--team <team.json>`），为其中每位成员生成一段
//! **完整的"加入团队"提示词**（可直接复制分发给对应 agent）；提示词同时：
//! ① 打印到会话（可复制的代码块）；② 落盘 `<data-dir>/team-prompts.md`；
//! ③ 通过 Web 只读面板 `/team` 页面与 `/api/team` 接口展示（代码区 + 一键复制）。

use std::path::PathBuf;

use serde_json::Value;

/// 团队成员。
#[derive(Debug, Clone)]
pub struct Member {
    /// 节点名（`--name`，全网唯一）。
    pub name: String,
    /// 角色（如 backend / frontend / reviewer）。
    pub role: String,
    /// 备注（可选，随提示词一起下发）。
    pub note: String,
}

/// 工作团队。
#[derive(Debug, Clone)]
pub struct Team {
    pub name: String,
    pub members: Vec<Member>,
}

/// 生成提示词所需的主节点运行上下文。
#[derive(Debug, Clone)]
pub struct JoinCtx {
    /// 主节点消息枢纽地址（如 `ws://127.0.0.1:10001`）。
    pub server: String,
    /// 加密模式（`plain` / `tls`，子节点须一致）。
    pub mode: String,
    /// 是否强制 JWT（子节点须携带 `--token`）。
    pub token_required: bool,
    /// 数据目录（inbox/outbox/wake.log 所在）。
    pub data_dir: String,
}

impl Team {
    /// 从 JSON 文件加载团队定义。
    ///
    /// 格式：
    /// ```json
    /// { "team": "dev-squad",
    ///   "members": [ { "name": "worker-a", "role": "backend", "note": "Rust 后端" } ] }
    /// ```
    pub fn load(path: &std::path::Path) -> Result<Team, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("读取团队文件 {} 失败: {e}", path.display()))?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| format!("团队文件 {} JSON 解析失败: {e}", path.display()))?;
        Self::from_value(&v)
    }

    /// 从 JSON 值构造团队（便于单测）。
    pub fn from_value(v: &Value) -> Result<Team, String> {
        let name = v
            .get("team")
            .and_then(|t| t.as_str())
            .unwrap_or("team")
            .to_string();
        let arr = v
            .get("members")
            .and_then(|m| m.as_array())
            .ok_or_else(|| "团队定义缺少 members 数组".to_string())?;
        if arr.is_empty() {
            return Err("团队成员（members）不能为空".to_string());
        }
        let mut members = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (i, m) in arr.iter().enumerate() {
            let name = m
                .get("name")
                .and_then(|n| n.as_str())
                .map(str::to_string)
                .ok_or_else(|| format!("第 {} 个成员缺少 name", i + 1))?;
            if !seen.insert(name.clone()) {
                return Err(format!("成员名重复：{name}（节点名称不可重复）"));
            }
            let role = m
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("worker")
                .to_string();
            let note = m
                .get("note")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            members.push(Member { name, role, note });
        }
        Ok(Team { name, members })
    }

    /// 团队配置模板（`--init-team` 写出，供用户填写）。
    pub fn template() -> &'static str {
        r#"{
  "team": "my-team",
  "members": [
    { "name": "worker-a", "role": "backend",  "note": "Rust 后端 / 数据" },
    { "name": "worker-b", "role": "frontend", "note": "Web UI / 面板" },
    { "name": "worker-c", "role": "reviewer", "note": "代码审查 / 测试" }
  ]
}
"#
    }

    /// 单个成员的加入提示词（携带已签发的真实令牌，复制即可接入）。
    pub fn prompt_with_token(&self, m: &Member, ctx: &JoinCtx, token: Option<&String>) -> String {
        self.prompt_of(m, ctx, token.map(|s| s.as_str()))
    }

    /// 单个成员的加入提示词。
    fn prompt_of(&self, m: &Member, ctx: &JoinCtx, token: Option<&str>) -> String {
        let node_data_dir = format!("~/.starfruit/nodes/{}", m.name);
        let (token_flag, token_step) = match token {
            Some(t) => (
                format!(" --token {t}"),
                format!("   JWT 令牌（已签发，直接使用）：{t}\n"),
            ),
            None if ctx.token_required => (
                " --token <令牌>".to_string(),
                format!(
                    "   JWT 令牌（--token）：由主节点签发 —— starfruit-master --issue-token {} --data-dir {}\n",
                    m.name, ctx.data_dir
                ),
            ),
            None => (String::new(), String::new()),
        };
        let note_line = if m.note.is_empty() {
            String::new()
        } else {
            format!("（备注：{}）", m.note)
        };
        format!(
            r#"你是 starfruit 工作团队「{team}」的成员「{name}」，角色：{role}{note}。
请让子节点长期后台运行并接入主节点，然后等待主节点派发任务。

【你的身份】
- 节点名（--name，全网唯一，重名会被拒绝 name_conflict）：{name}
- 角色：{role}
- 主节点（--server）：{server}
- 加密模式（--mode，必须与主节点一致）：{mode}
{token_step}
【1. 脱离会话后台启动（必须）】
按所在机器环境任选一种客户端实现（三者功能等价，协议相同）：
  Rust    : ./nodes/rust/target/release/starfruit-node --name {name} --mode {mode} --server {server} --data-dir {node_data_dir} --daemon{token_flag}
  Python3 : python3 nodes/python/starfruit_node.py --name {name} --mode {mode} --server {server} --data-dir {node_data_dir} --daemon{token_flag}
  Node.js : node nodes/nodejs/starfruit-node.js --name {name} --mode {mode} --server {server} --data-dir {node_data_dir} --daemon{token_flag}
进程必须以脱离本 agent 会话的方式运行（--daemon / nohup / systemd），
会话结束或 agent 退出都不得导致节点退出。数据统一存用户级目录 ~/.starfruit。

【2. 保活】
用 scripts/starfruit-keepalive.sh 监督该进程，或用 systemd（Restart=always）守护，
异常退出自动重启，保证节点长期在线。
（可选）若本机装有 cucumber（黄瓜）进程管理工具，可先征得用户同意，
再用 scripts/starfruit-cucumber.sh add 把本节点注册进去统一管理。

【3. 被动唤醒（复用 agent 的 hooks 机制，必须配置）】
运行 scripts/starfruit-wake-setup.sh {node_data_dir}/wake.log 自动对接当前 agent 工具：
它会检测本机 agent（如 Claude Code hooks）并注册常驻唤醒桥；其余环境以
STARFRUIT_WAKE_CMD='<你的 agent 唤醒命令>' 后台常驻 scripts/starfruit-wake-bridge.sh。
也可直接把节点 --wake-cmd 指向你的唤醒入口（拉起/恢复会话的 CLI）。

【4. 就绪上报】
注册成功后（日志出现 registered），向 manager 上报 ready：
  echo '{{"to":"manager","type":"status","text":"{name} ready"}}' >> {node_data_dir}/{name}.outbox.jsonl

请确认：节点已后台运行、保活已配置、唤醒钩子已就绪，然后开始待命。"#,
            team = self.name,
            name = m.name,
            role = m.role,
            note = note_line,
            server = ctx.server,
            mode = ctx.mode,
            node_data_dir = node_data_dir,
            token_step = token_step,
            token_flag = token_flag,
        )
    }

    /// 构建面板/文件视图（JSON + Markdown + 独立 HTML 页；携带已签发令牌）。
    pub fn view_with_tokens(
        &self,
        ctx: &JoinCtx,
        tokens: &std::collections::HashMap<String, String>,
    ) -> TeamView {
        let prompts: Vec<(String, String)> = self
            .members
            .iter()
            .map(|m| (m.name.clone(), self.prompt_with_token(m, ctx, tokens.get(&m.name))))
            .collect();
        let members_json: Vec<Value> = self
            .members
            .iter()
            .zip(prompts.iter())
            .map(|(m, (_, p))| {
                serde_json::json!({
                    "name": m.name,
                    "role": m.role,
                    "note": m.note,
                    "prompt": p,
                })
            })
            .collect();
        let json = serde_json::json!({
            "team": self.name,
            "server": ctx.server,
            "mode": ctx.mode,
            "token_required": ctx.token_required,
            "members": members_json,
        });

        let mut md = format!(
            "# starfruit 工作团队：{}\n\n> 主节点已为每位成员生成「加入团队」提示词，可分别复制给对应 agent。\n> 主节点：{}（--mode {}）\n\n",
            self.name, ctx.server, ctx.mode
        );
        let mut body = String::new();
        for (name, prompt) in &prompts {
            let role = self
                .members
                .iter()
                .find(|m| &m.name == name)
                .map(|m| m.role.as_str())
                .unwrap_or("worker");
            md.push_str(&format!("## 成员：{name}（{role}）\n\n```text\n{prompt}\n```\n\n"));
            body.push_str(&format!(
                "<section class=\"member\"><h2>{name} <span class=\"role\">{role}</span></h2>\
<button class=\"copy\" onclick=\"starfruitCopy(this)\">复制提示词</button>\
<pre><code>{code}</code></pre></section>\n",
                name = esc(name),
                role = esc(role),
                code = esc(prompt),
            ));
        }

        let html = format!(
            r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>starfruit 工作团队 · {team}</title>
<link rel="icon" type="image/svg+xml" href="/static/img/logo.svg">
<link rel="stylesheet" href="/style.css">
<style>
  body {{ max-width: 960px; margin: 0 auto; padding: 24px; }}
  .member {{ margin: 18px 0; padding: 16px; background: #141a20; border-radius: 10px; }}
  .member h2 {{ margin: 0 0 10px; font-size: 16px; }}
  .member .role {{ color: #7fd1c8; font-weight: 400; font-size: 13px; }}
  .copy {{ float: right; cursor: pointer; padding: 4px 12px; border-radius: 6px;
           border: 1px solid #2f7d74; background: #16302d; color: #9fe3da; font-size: 12px; }}
  pre {{ clear: both; overflow-x: auto; padding: 12px; background: #0b0f13; border-radius: 8px;
         border: 1px solid #223; white-space: pre-wrap; word-break: break-word; }}
  code {{ font-size: 12.5px; line-height: 1.55; }}
</style>
</head>
<body>
<h1>starfruit 工作团队 · {team}</h1>
<p>主节点：<code>{server}</code>（--mode {mode}）　成员数：{count}</p>
<p>将下列提示词分别复制给你的各个 agent，即可让其加入团队。</p>
{body}
<script>
function starfruitCopy(btn) {{
  var pre = btn.parentElement.querySelector('pre');
  var text = pre ? pre.innerText : '';
  navigator.clipboard.writeText(text).then(function () {{
    var t = btn.textContent;
    btn.textContent = '已复制';
    setTimeout(function () {{ btn.textContent = t; }}, 1200);
  }});
}}
</script>
</body>
</html>
"#,
            team = esc(&self.name),
            server = esc(&ctx.server),
            mode = esc(&ctx.mode),
            count = self.members.len(),
            body = body,
        );

        TeamView {
            json,
            markdown: md,
            html,
            prompts,
        }
    }
}

/// 面板/文件视图。
pub struct TeamView {
    /// `/api/team` 返回的 JSON。
    pub json: Value,
    /// `team-prompts.md` 的内容。
    pub markdown: String,
    /// `/team` 页面 HTML。
    pub html: String,
    /// 各成员的「加入团队」提示词（终端打印与落盘共用）。
    pub prompts: Vec<(String, String)>,
}

/// 最小 HTML 转义。
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 团队提示词在会话中的打印用（返回多行字符串，外层加可复制分隔）。
pub fn print_prompts(team: &Team, prompts: &[(String, String)]) {
    let sep = "────────────────────────────────────────────────────────────────────";
    eprintln!("{sep}");
    eprintln!("工作团队「{}」：已为 {} 位成员生成「加入团队」提示词（可直接复制）", team.name, prompts.len());
    eprintln!("提示词同时落盘：<data-dir>/team-prompts.md，并可访问面板 /team 查看。");
    for (name, prompt) in prompts {
        eprintln!("{sep}");
        eprintln!("```text  ← 成员 {name} 的加入提示词（复制开始）");
        for line in prompt.lines() {
            eprintln!("{line}");
        }
        eprintln!("```      ← 复制结束");
    }
    eprintln!("{sep}");
}

/// 团队提醒（未配置团队时打印，提示用户可选配置）。
pub fn print_reminder() {
    let sep = "────────────────────────────────────────────────────────────────────";
    eprintln!("{sep}");
    eprintln!("工作团队（可选）：");
    eprintln!("  需要多执行者分工协作时，可为主节点配置一个工作团队；主节点会为每位成员");
    eprintln!("  自动生成「加入团队」的完整提示词（终端可复制 + 落盘 team-prompts.md + 面板 /team 查看）。");
    eprintln!("  · 生成模板：starfruit-master --init-team team.json");
    eprintln!("  · 加载团队：starfruit-master --team team.json（其余参数照常）");
    eprintln!("{sep}");
}

/// 默认提示词文件路径（`<data-dir>/team-prompts.md`）。
pub fn prompts_path(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("team-prompts.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> JoinCtx {
        JoinCtx {
            server: "ws://127.0.0.1:10001".into(),
            mode: "plain".into(),
            token_required: false,
            data_dir: "/tmp/sf".into(),
        }
    }

    #[test]
    fn parse_team_and_generate_prompts() {
        let v: Value = serde_json::from_str(
            r#"{"team":"squad","members":[{"name":"a","role":"backend"},{"name":"b","role":"ui","note":"x"}]}"#,
        )
        .unwrap();
        let t = Team::from_value(&v).unwrap();
        assert_eq!(t.name, "squad");
        assert_eq!(t.members.len(), 2);
        let prompts: Vec<String> = t
            .members
            .iter()
            .map(|m| t.prompt_with_token(m, &ctx(), None))
            .collect();
        assert_eq!(prompts.len(), 2);
        assert!(prompts[0].contains("--name a"));
        assert!(prompts[1].contains("角色：ui"));
        assert!(prompts[1].contains("备注：x"));
        // 提示词包含后台运行 / 保活 / hooks 被动唤醒三段要点。
        assert!(prompts[0].contains("--daemon"));
        assert!(prompts[0].contains("starfruit-keepalive.sh"));
        assert!(prompts[0].contains("starfruit-wake-bridge.sh"));
    }

    #[test]
    fn token_prompt_includes_token_flag() {
        let v: Value =
            serde_json::from_str(r#"{"team":"t","members":[{"name":"a"}]}"#).unwrap();
        let t = Team::from_value(&v).unwrap();
        let mut c = ctx();
        c.token_required = true;
        let p = t.prompt_with_token(&t.members[0], &c, None);
        assert!(p.contains("--token <令牌>"));
        assert!(p.contains("--issue-token a"));
    }

    #[test]
    fn empty_members_rejected() {
        let v: Value = serde_json::from_str(r#"{"team":"t","members":[]}"#).unwrap();
        assert!(Team::from_value(&v).is_err());
    }

    #[test]
    fn 成员名重复应被拒绝() {
        let v: Value = serde_json::from_str(
            r#"{"team":"t","members":[{"name":"a"},{"name":"a"}]}"#,
        )
        .unwrap();
        let err = Team::from_value(&v).unwrap_err();
        assert!(err.contains("重复"));
    }

    #[test]
    fn 携带真实令牌的提示词() {
        let v: Value =
            serde_json::from_str(r#"{"team":"t","members":[{"name":"a"}]}"#).unwrap();
        let t = Team::from_value(&v).unwrap();
        let mut c = ctx();
        c.token_required = true;
        let token = "tok-xyz".to_string();
        let p = t.prompt_with_token(&t.members[0], &c, Some(&token));
        assert!(p.contains("--token tok-xyz"));
        assert!(p.contains("已签发"));
        assert!(!p.contains("<令牌>"));
        assert!(p.contains("~/.starfruit/nodes/a"));
        assert!(p.contains("starfruit-wake-setup.sh"));
    }

    #[test]
    fn view_has_json_markdown_and_html() {
        let v: Value =
            serde_json::from_str(r#"{"team":"t","members":[{"name":"a","role":"backend"}]}"#)
                .unwrap();
        let t = Team::from_value(&v).unwrap();
        let view = t.view_with_tokens(&ctx(), &std::collections::HashMap::new());
        assert_eq!(view.json["members"][0]["name"], "a");
        assert!(view.markdown.contains("```text"));
        assert!(view.html.contains("starfruitCopy"));
    }
}
