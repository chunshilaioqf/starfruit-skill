//! 首次启动向导：部署模式 → 团队创建（名称唯一）→ 令牌有效期 → 初始化落盘。
//!
//! 首启（data-dir 无 setup.json）且 stdin 为终端时自动进入；`--setup` 强制进入
//! （可从管道喂答案，供脚本/e2e 使用）；`--no-setup` 跳过。
//! 选择结果持久化 `<data-dir>/setup.json`（团队另存 `team.json`），后续启动自动复用。

use std::io::BufRead;
use std::path::Path;

use serde_json::Value;

/// 向导结果（与 setup.json 一一对应）。
#[derive(Debug, Clone)]
pub struct Setup {
    pub deploy: String,
    pub mode: String,
    pub hub_addr: String,
    pub web_addr: String,
    pub jwt_enabled: bool,
    pub jwt_ttl: u64,
    pub team: Option<Value>,
    pub nodes_doc: Option<String>,
}

impl Setup {
    pub fn load(data_dir: &Path) -> Option<Setup> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(data_dir.join("setup.json")).ok()?).ok()?;
        Some(Setup {
            deploy: v["deploy"].as_str().unwrap_or("local").to_string(),
            mode: v["mode"].as_str().unwrap_or("plain").to_string(),
            hub_addr: v["hub_addr"].as_str().unwrap_or("127.0.0.1:10001").to_string(),
            web_addr: v["web_addr"].as_str().unwrap_or("127.0.0.1:10002").to_string(),
            jwt_enabled: v["jwt_enabled"].as_bool().unwrap_or(false),
            jwt_ttl: v["jwt_ttl"].as_u64().unwrap_or(86400),
            team: v.get("team").filter(|t| !t.is_null()).cloned(),
            nodes_doc: v["nodes_doc"].as_str().map(str::to_string),
        })
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), String> {
        let v = serde_json::json!({
            "deploy": self.deploy,
            "mode": self.mode,
            "hub_addr": self.hub_addr,
            "web_addr": self.web_addr,
            "jwt_enabled": self.jwt_enabled,
            "jwt_ttl": self.jwt_ttl,
            "team": self.team,
            "nodes_doc": self.nodes_doc,
            "created_at_ms": crate::protocol::now_ms(),
        });
        std::fs::write(data_dir.join("setup.json"), serde_json::to_string_pretty(&v).unwrap_or_default())
            .map_err(|e| format!("写入 setup.json 失败: {e}"))?;
        if let Some(team) = &self.team {
            std::fs::write(
                data_dir.join("team.json"),
                serde_json::to_string_pretty(team).unwrap_or_default(),
            )
            .map_err(|e| format!("写入 team.json 失败: {e}"))?;
        }
        Ok(())
    }
}

/// 部署模式选项解析（空 = 默认 local）。
pub fn parse_deploy_choice(s: &str) -> Option<&'static str> {
    match s.trim() {
        "" | "1" | "local" => Some("local"),
        "2" | "lan" => Some("lan"),
        "3" | "public" => Some("public"),
        _ => None,
    }
}

/// 令牌有效期选项：1=24h 2=7d 3=30d 4=365d；其它数字=自定义秒数；空=默认 24h。
pub fn parse_ttl_choice(s: &str) -> Option<u64> {
    match s.trim() {
        "" | "1" => Some(86400),
        "2" => Some(7 * 86400),
        "3" => Some(30 * 86400),
        "4" => Some(365 * 86400),
        other => other.parse::<u64>().ok().filter(|n| *n >= 60),
    }
}

/// 成员行解析：`名称 [角色] [备注...]`；空行返回 None（结束输入）。
pub fn parse_member_line(s: &str) -> Option<(String, String, String)> {
    let mut it = s.trim().splitn(3, char::is_whitespace);
    let name = it.next()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let role = it.next().unwrap_or("worker").trim().to_string();
    let note = it.next().unwrap_or("").trim().to_string();
    Some((name, if role.is_empty() { "worker".into() } else { role }, note))
}

/// y/n 问题（空 = 默认值）。
pub fn parse_yes_no(s: &str, default: bool) -> bool {
    match s.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "y" | "yes" | "是" => true,
        "n" | "no" | "否" => false,
        _ => default,
    }
}

/// 交互式向导（stdin 逐行读答案；EOF 时取默认值，不会挂起）。
pub fn run_wizard(manager_name: &str) -> Setup {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut ask = |prompt: &str, default: &str| -> String {
        if default.is_empty() {
            eprint!("{prompt}");
        } else {
            eprint!("{prompt}（默认 {default}）");
        }
        let _ = std::io::Write::flush(&mut std::io::stderr());
        match lines.next() {
            Some(Ok(l)) if !l.trim().is_empty() => l.trim().to_string(),
            _ => default.to_string(),
        }
    };

    let sep = "────────────────────────────────────────────────────────────────────";
    eprintln!("{sep}");
    eprintln!("⭐ starfruit 首次启动向导（选择将保存到 data-dir/setup.json，之后启动自动复用）");
    eprintln!("{sep}");

    // ① 部署模式。
    let deploy = loop {
        let a = ask("① 部署模式：[1] local 本机（默认） [2] lan 局域网 [3] public 公网（Cloudflare Tunnel）\n   请选择", "1");
        if let Some(d) = parse_deploy_choice(&a) {
            break d;
        }
        eprintln!("   无效选项，请输入 1/2/3。");
    };
    let (hub_addr, web_addr, mode) = match deploy {
        "lan" => {
            let ip = loop {
                let a = ask("② 本机局域网 IP（如 192.168.1.10，禁止 0.0.0.0/127.0.0.1）", "");
                if !a.is_empty() && !a.starts_with("127.") && a != "localhost" && a != "0.0.0.0" && a != "::" {
                    break a;
                }
                eprintln!("   需要具体的局域网 IP。");
            };
            let m = ask("③ 加密模式 plain/tls", "plain");
            (format!("{ip}:10001"), format!("{ip}:10002"), if m == "tls" { "tls" } else { "plain" })
        }
        "public" => {
            eprintln!("② 公网模式：监听 127.0.0.1:10001 作为 Cloudflare Tunnel 指向；强制 tls + JWT。");
            eprintln!("   Tunnel 由 scripts/starfruit-tunnel.sh 独立后台运行，主节点重启不影响公网地址。");
            ("127.0.0.1:10001".to_string(), "127.0.0.1:10002".to_string(), "tls")
        }
        _ => {
            let m = ask("② 加密模式 plain/tls", "plain");
            ("127.0.0.1:10001".to_string(), "127.0.0.1:10002".to_string(), if m == "tls" { "tls" } else { "plain" })
        }
    };

    // ② JWT 与有效期。
    let jwt_enabled = if deploy == "public" {
        true
    } else {
        parse_yes_no(&ask("③ 启用 JWT 令牌校验（子节点须携带令牌）？[Y/n]", "Y"), true)
    };
    let jwt_ttl = if jwt_enabled {
        loop {
            let a = ask("④ 令牌有效期：[1] 24 小时（默认） [2] 7 天 [3] 30 天 [4] 365 天 [其它数字] 自定义秒数", "1");
            if let Some(t) = parse_ttl_choice(&a) {
                break t;
            }
            eprintln!("   无效选项（自定义秒数须 ≥60）。");
        }
    } else {
        86400
    };

    // ③ 工作团队（成员名全网唯一）。
    let mut team: Option<Value> = None;
    if parse_yes_no(&ask("⑤ 创建工作团队（为每位成员签发令牌并生成加入提示词）？[Y/n]", "Y"), true) {
        let team_name = ask("   团队名称", "my-team");
        eprintln!("   逐个输入成员：`名称 [角色] [备注]`（名称不可重复，且不能与主节点名 {manager_name} 相同）；空行结束。");
        let mut members: Vec<Value> = Vec::new();
        loop {
            let line = ask("   成员", "");
            let Some((name, role, note)) = parse_member_line(&line) else { break };
            if name == manager_name {
                eprintln!("   ✗ 名称 {name} 与主节点管理者名冲突，请换一个。");
                continue;
            }
            if members.iter().any(|m| m["name"].as_str() == Some(name.as_str())) {
                eprintln!("   ✗ 名称 {name} 重复（节点名称不可重复），请换一个。");
                continue;
            }
            members.push(serde_json::json!({ "name": name, "role": role, "note": note }));
        }
        if members.is_empty() {
            eprintln!("   未输入成员，跳过团队创建（之后可用 --team 加载）。");
        } else {
            team = Some(serde_json::json!({ "team": team_name, "members": members }));
        }
    }

    // ④ 节点文档路径。
    let doc = ask("⑥ starfruit-nodes.md 保存路径（回车=自动：docs/ > doc/ > 当前目录）", "");
    let nodes_doc = if doc.is_empty() { None } else { Some(doc) };

    let setup = Setup {
        deploy: deploy.to_string(),
        mode: mode.to_string(),
        hub_addr,
        web_addr,
        jwt_enabled,
        jwt_ttl,
        team,
        nodes_doc,
    };
    eprintln!("{sep}");
    eprintln!("✓ 向导完成：deploy={deploy} mode={mode} hub={} jwt={} ttl={jwt_ttl}s 团队={}",
        setup.hub_addr, jwt_enabled,
        setup.team.as_ref().and_then(|t| t["team"].as_str()).unwrap_or("未创建"));
    eprintln!("{sep}");
    setup
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 部署选项解析() {
        assert_eq!(parse_deploy_choice(""), Some("local"));
        assert_eq!(parse_deploy_choice("2"), Some("lan"));
        assert_eq!(parse_deploy_choice("public"), Some("public"));
        assert_eq!(parse_deploy_choice("9"), None);
    }

    #[test]
    fn 有效期选项解析() {
        assert_eq!(parse_ttl_choice(""), Some(86400));
        assert_eq!(parse_ttl_choice("2"), Some(604800));
        assert_eq!(parse_ttl_choice("3600"), Some(3600));
        assert_eq!(parse_ttl_choice("30"), None); // <60 秒拒绝
        assert_eq!(parse_ttl_choice("abc"), None);
    }

    #[test]
    fn 成员行解析() {
        assert_eq!(
            parse_member_line("worker-a backend Rust 后端"),
            Some(("worker-a".into(), "backend".into(), "Rust 后端".into()))
        );
        assert_eq!(parse_member_line("solo"), Some(("solo".into(), "worker".into(), "".into())));
        assert_eq!(parse_member_line("   "), None);
    }

    #[test]
    fn setup_落盘与读回() {
        let dir = std::env::temp_dir().join(format!("sf-setup-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = Setup {
            deploy: "public".into(),
            mode: "tls".into(),
            hub_addr: "127.0.0.1:10001".into(),
            web_addr: "127.0.0.1:10002".into(),
            jwt_enabled: true,
            jwt_ttl: 604800,
            team: Some(serde_json::json!({"team":"t","members":[{"name":"a"}]})),
            nodes_doc: Some("/tmp/nodes.md".into()),
        };
        s.save(&dir).unwrap();
        let back = Setup::load(&dir).unwrap();
        assert_eq!(back.deploy, "public");
        assert_eq!(back.jwt_ttl, 604800);
        assert_eq!(back.team.unwrap()["team"], "t");
        assert!(dir.join("team.json").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}