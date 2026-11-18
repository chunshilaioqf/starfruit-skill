//! starfruit-nodes.md：节点接入文档的生成与动态更新。
//!
//! 首启生成；之后节点上线/下线、令牌签发、Tunnel 地址变化时自动重写。
//! 内容：主节点地址端口、面板地址、公网 Tunnel 地址（如有）、团队成员名/角色/令牌，
//! 以及每位成员/节点的连接提示词（```text 代码块，复制即可发给对应子节点 agent）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::protocol::State;
use crate::team::Team;
use crate::update::{CURRENT_VERSION, REPO_URL};

/// 文档渲染上下文（启动时确定）。
#[derive(Clone)]
pub struct DocCtx {
    pub doc_path: PathBuf,
    pub manager_name: String,
    pub deploy: String,
    pub mode: String,
    pub server: String,
    pub web: String,
    pub data_dir: String,
    pub team: Option<Team>,
    pub jwt: bool,
    /// `--tunnel-url` 显式指定（缺省读 `~/.starfruit/tunnel/url`）。
    pub tunnel_url: Option<String>,
}

impl DocCtx {
    fn tunnel(&self) -> Option<String> {
        self.tunnel_url
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(crate::paths::read_tunnel_url)
    }
}

/// 文档路径决策：显式指定 > 当前目录 docs/ > doc/ > 根目录。
pub fn resolve_doc_path(explicit: Option<&str>) -> PathBuf {
    if let Some(p) = explicit {
        return PathBuf::from(p);
    }
    for dir in ["docs", "doc"] {
        if Path::new(dir).is_dir() {
            return Path::new(dir).join("starfruit-nodes.md");
        }
    }
    PathBuf::from("starfruit-nodes.md")
}

/// 把 starfruit-nodes.md 加入 .gitignore（当前目录已有则追加；无 .gitignore 但在
/// git 仓库中则创建；否则不动）。
pub fn ensure_gitignore() {
    let entry = "starfruit-nodes.md";
    let path = Path::new(".gitignore");
    if path.exists() {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if text.lines().any(|l| l.trim() == entry) {
            return;
        }
        let mut text = text;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("{entry}\n"));
        let _ = std::fs::write(path, text);
    } else if Path::new(".git").exists() {
        let _ = std::fs::write(path, format!("{entry}\n"));
    }
}

/// 读取令牌表（`<data-dir>/tokens.json`，0600）。
pub fn load_tokens(data_dir: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(data_dir.join("tokens.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .map(|m| {
            m.into_iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// 写入令牌表（0600；合并已有条目）。
pub fn save_tokens(data_dir: &Path, tokens: &HashMap<String, String>) {
    let mut all = load_tokens(data_dir);
    all.extend(tokens.iter().map(|(k, v)| (k.clone(), v.clone())));
    let path = data_dir.join("tokens.json");
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&all).unwrap_or_default());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

/// 渲染文档全文。
pub async fn render(state: &Arc<State>, ctx: &DocCtx, tokens: &HashMap<String, String>) -> String {
    let reg = state.registry.lock().await;
    let online = |name: &str| reg.get(name).map(|m| m.online).unwrap_or(false);
    let tunnel = ctx.tunnel();
    let now = crate::protocol::now_ms();

    let mut md = format!(
        "# starfruit 节点接入文档\n\n\
         > 自动生成并随节点信息变动更新 · 最近写入：{ts} · 版本 {CURRENT_VERSION}\n\
         > 仓库：{REPO_URL}\n\
         > ⚠️ 本文档含 JWT 令牌，已加入 .gitignore，请勿提交或外传。\n\n\
         ## 主节点\n\n\
         | 项 | 值 |\n|---|---|\n\
         | 管理者名 | `{manager}` |\n\
         | 部署模式 | `{deploy}` |\n\
         | 运行模式 | `{mode}`（子节点 --mode 必须一致） |\n\
         | 消息枢纽 | `{server}` |\n\
         | 只读面板 | {web} |\n",
        ts = fmt_ts(now),
        manager = ctx.manager_name,
        deploy = ctx.deploy,
        mode = ctx.mode,
        server = ctx.server,
        web = ctx.web,
    );
    if let Some(url) = &tunnel {
        md.push_str(&format!(
            "| 公网地址（Cloudflare Tunnel，独立后台运行） | `{url}` |\n"
        ));
    }
    md.push_str(&format!(
        "| JWT 校验 | {} |\n| 数据目录 | `{}` |\n\n",
        if ctx.jwt { "已启用（令牌见下表）" } else { "未启用" },
        ctx.data_dir
    ));

    // 团队成员表 + 每成员连接提示词（含真实令牌）。
    if let Some(team) = &ctx.team {
        md.push_str(&format!("## 工作团队：{}\n\n", team.name));
        md.push_str("| 成员 | 角色 | 在线 | 令牌 |\n|---|---|---|---|\n");
        for m in &team.members {
            let token = tokens.get(&m.name).map(|t| format!("`{t}`")).unwrap_or_else(|| "–".into());
            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                m.name,
                m.role,
                if online(&m.name) { "🟢 在线" } else { "⚪ 离线" },
                token
            ));
        }
        md.push('\n');
        let join_ctx = crate::team::JoinCtx {
            server: ctx.server.clone(),
            mode: ctx.mode.clone(),
            token_required: ctx.jwt,
            data_dir: ctx.data_dir.clone(),
        };
        for m in &team.members {
            let prompt = team.prompt_with_token(m, &join_ctx, tokens.get(&m.name));
            md.push_str(&format!(
                "### 成员 {} 的连接提示词（复制发给对应 agent）\n\n```text\n{prompt}\n```\n\n",
                m.name
            ));
        }
    }

    // 其它已注册节点（非团队成员）。
    let team_names: Vec<&str> = ctx
        .team
        .as_ref()
        .map(|t| t.members.iter().map(|m| m.name.as_str()).collect())
        .unwrap_or_default();
    let mut others: Vec<(&String, &crate::protocol::NodeMeta)> = reg
        .iter()
        .filter(|(name, m)| !m.internal && !team_names.contains(&name.as_str()))
        .collect();
    others.sort_by_key(|(n, _)| n.to_string());
    if !others.is_empty() {
        md.push_str("## 其它已注册节点\n\n| 节点 | 角色 | 在线 | 收 | 发 |\n|---|---|---|---|---|\n");
        for (name, m) in &others {
            md.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                name,
                m.role,
                if m.online { "🟢" } else { "⚪" },
                m.msgs_in,
                m.msgs_out
            ));
        }
        md.push('\n');
        for (name, _) in &others {
            let token_flag = tokens
                .get(*name)
                .map(|t| format!(" --token {t}"))
                .unwrap_or_else(|| if ctx.jwt { " --token <令牌>".into() } else { String::new() });
            md.push_str(&format!(
                "### 节点 {name} 的连接命令\n\n```bash\n\
                 starfruit-node --name {name} --mode {mode} --server {server}{token_flag} --daemon\n\
                 # 或 python3 starfruit_node.py / node starfruit-node.js（参数相同，功能等价）\n```\n\n",
                mode = ctx.mode,
                server = ctx.server,
            ));
        }
    }

    md.push_str(&format!(
        "## 运维说明\n\n\
         - 节点名称**全网唯一**：创建与登录均校验，重名在线连接会被拒绝（name_conflict）。\n\
         - 所有节点须脱离会话后台运行（--daemon）并自建保活（scripts/starfruit-keepalive.sh）。\n\
         - 被动唤醒：scripts/starfruit-wake-setup.sh 可把唤醒桥注册为 agent 常驻 hook。\n\
         - 公网 Tunnel 独立后台运行：scripts/starfruit-tunnel.sh start|stop|status|url；\n\
           主节点重启不影响公网地址，仅显式 stop/restart 时才变更。\n\
         - 更新检查：每周一次（仓库 {REPO_URL}）；更新用 scripts/starfruit-update.sh（含数据迁移检查）。\n"
    ));
    md
}

/// 写盘（内容无变化时跳过）。
pub async fn write_if_changed(state: &Arc<State>, ctx: &DocCtx) {
    let tokens = load_tokens(Path::new(&ctx.data_dir));
    let md = render(state, ctx, &tokens).await;
    let old = std::fs::read_to_string(&ctx.doc_path).unwrap_or_default();
    // 时间戳每次都变：比较时去掉时间戳行。
    let strip_ts = |s: &str| -> String {
        s.lines().filter(|l| !l.contains("最近写入")).collect::<Vec<_>>().join("\n")
    };
    if strip_ts(&old) == strip_ts(&md) {
        return;
    }
    if let Some(parent) = ctx.doc_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&ctx.doc_path, &md) {
        Ok(_) => eprintln!("[nodes-doc] 已更新 {}", ctx.doc_path.display()),
        Err(e) => eprintln!("[nodes-doc] 写入 {} 失败: {e}", ctx.doc_path.display()),
    }
}

/// 后台更新任务：节点上线/下线事件即时触发；另每 5 秒比对令牌/Tunnel 变化。
pub fn spawn_updater(state: Arc<State>, ctx: DocCtx) {
    tokio::spawn(async move {
        let mut rx = state.events.subscribe();
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
        write_if_changed(&state, &ctx).await;
        loop {
            tokio::select! {
                ev = rx.recv() => {
                    if let Ok(v) = ev {
                        let kind = v["ev"].as_str().unwrap_or("");
                        if kind == "node_join" || kind == "node_leave" {
                            write_if_changed(&state, &ctx).await;
                        }
                    }
                }
                _ = tick.tick() => {
                    write_if_changed(&state, &ctx).await;
                }
            }
        }
    });
}

/// epoch ms → `YYYY-MM-DD HH:MM:SS`（UTC，与节点日志一致）。
fn fmt_ts(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 文档路径决策_显式优先() {
        assert_eq!(resolve_doc_path(Some("/tmp/x.md")), PathBuf::from("/tmp/x.md"));
        // 无 docs/doc 目录的临时环境下落到根目录
        let p = resolve_doc_path(None);
        assert!(p.ends_with("starfruit-nodes.md"));
    }

    #[tokio::test]
    async fn 渲染含主节点信息与团队令牌() {
        let state = State::new(100, None);
        let team = Team::from_value(&serde_json::json!({
            "team": "squad", "members": [{"name":"worker-a","role":"backend"}]
        }))
        .unwrap();
        let ctx = DocCtx {
            doc_path: PathBuf::from("/tmp/starfruit-nodes.md"),
            manager_name: "manager".into(),
            deploy: "public".into(),
            mode: "tls".into(),
            server: "wss://127.0.0.1:10001".into(),
            web: "https://127.0.0.1:10002".into(),
            data_dir: "/tmp/sf".into(),
            team: Some(team),
            jwt: true,
            tunnel_url: Some("https://example.trycloudflare.com".into()),
        };
        let mut tokens = HashMap::new();
        tokens.insert("worker-a".to_string(), "tok-abc".to_string());
        let md = render(&state, &ctx, &tokens).await;
        assert!(md.contains("wss://127.0.0.1:10001"));
        assert!(md.contains("https://example.trycloudflare.com"));
        assert!(md.contains("tok-abc"));
        assert!(md.contains("```text"));
        assert!(md.contains("--name worker-a"));
        assert!(md.contains(REPO_URL));
    }
}