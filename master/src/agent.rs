//! 内置管理者端点：主节点自带一个"管理者"身份（不占用独立连接），
//! 提供 inbox/outbox 文件与 wake 唤醒钩子，供 agent 会话之外的后台对接。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::hub;
use crate::protocol::{now_ms, NodeMeta, State};

/// 管理者端点配置。
pub struct AgentCfg {
    pub name: String,
    pub data_dir: PathBuf,
    pub wake_cmd: Option<String>,
    pub wake_url: Option<String>,
}

/// wake 钩子配置（可移交后台任务持有）。
#[derive(Clone)]
pub struct WakeHooks {
    pub data_dir: PathBuf,
    pub wake_cmd: Option<String>,
    pub wake_url: Option<String>,
}

/// 启动内置管理者端点：
/// ① 注册进路由表（收消息 → inbox 文件 + wake 钩子）；
/// ② 轮询 outbox 文件（每行一条 JSON → 以管理者身份发出）。
pub async fn run(state: Arc<State>, cfg: AgentCfg) {
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    // 注册为在线节点（internal），接收定向/广播消息。
    {
        let mut map = state.peers.lock().await;
        map.insert(cfg.name.clone(), tx);
    }
    {
        let mut reg = state.registry.lock().await;
        reg.insert(
            cfg.name.clone(),
            NodeMeta::new("manager", "local", true),
        );
    }
    eprintln!(
        "[agent] 内置管理者端点已注册: {}（inbox/outbox 位于 {}）",
        cfg.name,
        cfg.data_dir.display()
    );
    state.emit(
        "node_join",
        serde_json::json!({ "name": cfg.name, "role": "manager", "ts_ms": now_ms() }),
    );

    let inbox = cfg.data_dir.join(format!("{}.inbox.jsonl", cfg.name));
    let outbox = cfg.data_dir.join(format!("{}.outbox.jsonl", cfg.name));
    {
        use std::fs::OpenOptions;
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&inbox)
            .map(|mut f| writeln!(f, r#"{{"system":true,"note":"inbox ready","ts_ms":{}}}"#, now_ms()));
    }

    // ① 收消息：落 inbox + wake 钩子。
    let hooks = WakeHooks {
        data_dir: cfg.data_dir.clone(),
        wake_cmd: cfg.wake_cmd.clone(),
        wake_url: cfg.wake_url.clone(),
    };
    let inbox_path = inbox.clone();
    let consumer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Message::Text(text) = msg {
                append_line(&inbox_path, &text);
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if kind == "wake" {
                    trigger_wake(&hooks, &v);
                }
            }
        }
    });

    // ② 发消息：轮询 outbox（追加式 JSONL，读后截断）。
    let state2 = state.clone();
    let name2 = cfg.name.clone();
    let producer = tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tick.tick().await;
            let lines = drain_file(&outbox);
            for line in lines {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let mut v: Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("[agent] outbox 行 JSON 解析失败: {e}");
                        continue;
                    }
                };
                v["from"] = Value::from(name2.as_str());
                let receipt = hub::route(&state2, &name2, &v).await;
                eprintln!("[agent] outbox 发出 → {}", receipt);
            }
        }
    });

    let _ = consumer.await;
    let _ = producer.await;
}

/// 追加一行到文件。
fn append_line(path: &Path, line: &str) {
    use std::fs::OpenOptions;
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// 读取并清空文件内容（outbox 消费）。
fn drain_file(path: &Path) -> Vec<String> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    if content.trim().is_empty() {
        return Vec::new();
    }
    let _ = std::fs::write(path, "");
    content.lines().map(str::to_string).collect()
}

/// wake 唤醒钩子：命令（带环境变量）/ HTTP POST / 哨兵文件（wake.log，始终写）。
pub fn trigger_wake(hooks: &WakeHooks, v: &Value) {
    let from = v.get("from").and_then(|f| f.as_str()).unwrap_or("");
    let to = v.get("to").and_then(|t| t.as_str()).unwrap_or("");
    let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("");
    let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");

    // 哨兵文件（默认）：agent 可 tail -f 触发对话。
    append_line(
        &hooks.data_dir.join("wake.log"),
        &serde_json::json!({ "from": from, "to": to, "text": text, "ts": ts, "ts_ms": now_ms() })
            .to_string(),
    );

    if let Some(cmd) = &hooks.wake_cmd {
        let status = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .env("SF_FROM", from)
            .env("SF_TO", to)
            .env("SF_TEXT", text)
            .env("SF_TS", ts)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => eprintln!("[wake] wake-cmd 退出码 {s}"),
            Err(e) => eprintln!("[wake] wake-cmd 执行失败: {e}"),
        }
    }

    if let Some(url) = &hooks.wake_url {
        let body = serde_json::json!({ "from": from, "to": to, "text": text, "ts": ts }).to_string();
        let resp = ureq::post(url)
            .set("content-type", "application/json")
            .send_string(&body);
        if let Err(e) = resp {
            eprintln!("[wake] wake-url POST 失败: {e}");
        }
    }
}
