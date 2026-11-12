//! starfruit-node：子节点（Rust 版）。
//!
//! 与 Python/Node.js 版功能等价（协议 docs/protocol.md v2）：
//! - 连接主节点（ws:// 明文 / wss:// 加密，自签证书默认信任）
//! - 注册（含 mode 声明；加密方式不对时服务端会向本节点报错）
//! - 收消息 → inbox.jsonl；type=wake → 唤醒钩子（wake-cmd / wake-url / wake.log）
//! - outbox.jsonl 追加式发消息（每行 {"to","type","text"}）
//! - --daemon 后台运行（脱离终端，pidfile + 日志）

use std::path::PathBuf;
use std::process::Command;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;

const BACKOFF: [u64; 5] = [2, 4, 8, 16, 30];

const HELP: &str = r#"starfruit 子节点（Rust 版）
用法: starfruit-node --name <名字> [选项]

选项:
  --server <url>     主节点地址 ws:// 或 wss://（默认 ws://127.0.0.1:9501）
  --name <name>      节点名（全网唯一，必填）
  --role <role>      角色标签（默认 worker）
  --mode plain|tls   加密模式，必须与主节点一致（默认 plain）
  --data-dir <dir>   运行数据目录（默认 ./starfruit-data）
  --wake-cmd <sh>    wake 唤醒钩子：shell 命令（SF_FROM/SF_TO/SF_TEXT/SF_TS）
  --wake-url <url>   wake 唤醒钩子：HTTP POST JSON
  --ca-cert <path>   TLS CA 证书（缺省信任自签）
  --daemon           后台运行
  -h, --help         显示帮助
"#;

struct Cfg {
    server: String,
    name: String,
    role: String,
    mode: String,
    data_dir: PathBuf,
    wake_cmd: Option<String>,
    wake_url: Option<String>,
    ca_cert: Option<PathBuf>,
    daemon: bool,
    daemon_child: bool,
}

fn parse_args() -> Cfg {
    let mut cfg = Cfg {
        server: "ws://127.0.0.1:9501".into(),
        name: String::new(),
        role: "worker".into(),
        mode: "plain".into(),
        data_dir: PathBuf::from("starfruit-data"),
        wake_cmd: None,
        wake_url: None,
        ca_cert: None,
        daemon: false,
        daemon_child: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val =
            |name: &str| args.next().unwrap_or_else(|| panic!("{name} 缺少参数值"));
        match a.as_str() {
            "--server" => cfg.server = val("--server"),
            "--name" => cfg.name = val("--name"),
            "--role" => cfg.role = val("--role"),
            "--mode" => cfg.mode = val("--mode"),
            "--data-dir" => cfg.data_dir = PathBuf::from(val("--data-dir")),
            "--wake-cmd" => cfg.wake_cmd = Some(val("--wake-cmd")),
            "--wake-url" => cfg.wake_url = Some(val("--wake-url")),
            "--ca-cert" => cfg.ca_cert = Some(PathBuf::from(val("--ca-cert"))),
            "--daemon" => cfg.daemon = true,
            "--_daemon-child" => cfg.daemon_child = true,
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            other => panic!("未知参数: {other}（--help 查看用法）"),
        }
    }
    if cfg.name.is_empty() {
        print!("{HELP}");
        std::process::exit(1);
    }
    if cfg.mode != "plain" && cfg.mode != "tls" {
        panic!("--mode 只能是 plain|tls");
    }
    cfg
}

fn log(msg: &str) {
    eprintln!("[{}] [starfruit-node] {msg}", fmt_ts(now_ms()));
}

/// 后台化：setsid 重拉自身，stdio 重定向日志，父进程退出（与主节点同语义）。
fn daemonize(cfg: &Cfg) {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().expect("无法定位自身可执行文件");
    std::fs::create_dir_all(&cfg.data_dir).expect("创建 data-dir 失败");
    let log_path = cfg.data_dir.join(format!("{}.log", cfg.name));
    let out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("打开日志文件失败");
    let err = out.try_clone().expect("克隆日志句柄失败");
    let mut cmd = Command::new(exe);
    cmd.args(std::env::args().skip(1))
        .arg("--_daemon-child")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(out))
        .stderr(std::process::Stdio::from(err));
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = cmd.spawn().expect("后台启动失败");
    let pid_file = cfg.data_dir.join(format!("{}.pid", cfg.name));
    let _ = std::fs::write(&pid_file, child.id().to_string());
    println!(
        "starfruit-node {} 已后台启动（pid {}），日志: {}",
        cfg.name,
        child.id(),
        log_path.display()
    );
    std::process::exit(0);
}

// ─────────────────────────── TLS（客户端） ───────────────────────────

/// LAN 自签证书默认信任的校验器。
#[derive(Debug)]
struct AcceptAny;

impl rustls::client::danger::ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn build_tls_config(ca_cert: Option<&PathBuf>) -> Result<rustls::ClientConfig, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    match ca_cert {
        None => Ok(rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(AcceptAny))
            .with_no_client_auth()),
        Some(path) => {
            let pem = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let mut roots = rustls::RootCertStore::empty();
            for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
                let cert = cert.map_err(|e| e.to_string())?;
                roots.add(cert).map_err(|e| e.to_string())?;
            }
            Ok(rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth())
        }
    }
}

// ─────────────────────────── 工具 ───────────────────────────

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// UTC 时间戳（epoch ms → `YYYY-MM-DD HH:MM:SS`）。
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
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

fn append_line(path: &std::path::Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

fn drain_file(path: &std::path::Path) -> Vec<String> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    if content.trim().is_empty() {
        return Vec::new();
    }
    let _ = std::fs::write(path, "");
    content.lines().map(str::to_string).collect()
}

/// wake 唤醒钩子：wake.log（始终）+ wake-cmd（环境变量）+ wake-url（POST JSON）。
fn trigger_wake(cfg: &Cfg, v: &Value) {
    let from = v.get("from").and_then(|f| f.as_str()).unwrap_or("");
    let to = v.get("to").and_then(|t| t.as_str()).unwrap_or("");
    let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("");
    let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("");

    append_line(
        &cfg.data_dir.join("wake.log"),
        &serde_json::json!({ "from": from, "to": to, "text": text, "ts": ts, "ts_ms": now_ms() })
            .to_string(),
    );
    if let Some(cmd) = &cfg.wake_cmd {
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
            Ok(s) => log(&format!("wake-cmd 退出码 {s}")),
            Err(e) => log(&format!("wake-cmd 执行失败: {e}")),
        }
    }
    if let Some(url) = &cfg.wake_url {
        let body =
            serde_json::json!({ "from": from, "to": to, "text": text, "ts": ts }).to_string();
        if let Err(e) = ureq::post(url)
            .set("content-type", "application/json")
            .send_string(&body)
        {
            log(&format!("wake-url POST 失败: {e}"));
        }
    }
}

// ─────────────────────────── 连接与主循环 ───────────────────────────

fn main() {
    let cfg = parse_args();
    std::fs::create_dir_all(&cfg.data_dir).expect("创建 data-dir 失败");
    if cfg.daemon && !cfg.daemon_child {
        daemonize(&cfg);
    }
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("构建 tokio runtime 失败");
    rt.block_on(run_forever(cfg));
}

async fn run_forever(cfg: Cfg) {
    log(&format!(
        "启动: name={} mode={} server={}",
        cfg.name, cfg.mode, cfg.server
    ));
    let mut attempt = 0usize;
    loop {
        match connect_once(&cfg).await {
            Ok(()) => attempt = 0,
            Err(e) => {
                log(&format!("连接中断: {e}"));
                if e.contains("主节点拒绝连接")
                    || e.contains("mode_mismatch")
                    || e.contains("TLS 握手失败")
                {
                    log("加密模式不匹配，请与主节点保持相同的 --mode 后重试");
                    std::process::exit(2);
                }
            }
        }
        let delay = BACKOFF[attempt.min(BACKOFF.len() - 1)];
        attempt += 1;
        log(&format!("{delay}s 后重连…"));
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
    }
}

type WsStream<S> = tokio_tungstenite::WebSocketStream<S>;

/// 泛型会话：TLS/明文分支的 WebSocketStream 类型不同，统一在泛型函数内处理。
async fn run_session<S>(cfg: &Cfg, ws: WsStream<S>) -> Result<(), String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut incoming) = ws.split();

    // 注册（v2：声明 mode）。
    let reg = serde_json::json!({
        "action": "register", "from": cfg.name,
        "role": cfg.role, "mode": cfg.mode, "ver": 2,
    })
    .to_string();
    sink.send(Message::text(reg))
        .await
        .map_err(|e| format!("注册发送失败: {e}"))?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let inbox_path = cfg.data_dir.join(format!("{}.inbox.jsonl", cfg.name));
    let outbox_path = cfg.data_dir.join(format!("{}.outbox.jsonl", cfg.name));

    // outbox 轮询。
    let tx_clone = tx.clone();
    let name = cfg.name.clone();
    let outbox_task = tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tick.tick().await;
            for line in drain_file(&outbox_path) {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let mut v: Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(e) => {
                        log(&format!("outbox 行 JSON 解析失败: {e}"));
                        continue;
                    }
                };
                v["from"] = Value::from(name.as_str());
                v["type"] =
                    Value::from(v.get("type").and_then(|t| t.as_str()).unwrap_or("chat"));
                v["ts"] = Value::from(v.get("ts").and_then(|t| t.as_str()).unwrap_or(""));
                v["text"] = Value::from(v.get("text").and_then(|t| t.as_str()).unwrap_or(""));
                let _ = tx_clone.send(Message::text(v.to_string()));
            }
        }
    });

    // 写任务：outbox / ping → 服务端。
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    // 读循环（周期 ping 保活）。
    let mut ping = tokio::time::interval(std::time::Duration::from_secs(30));
    ping.tick().await; // 首个 tick 立即完成，跳过
    let result: Result<(), String> = loop {
        tokio::select! {
            _ = ping.tick() => {
                let _ = tx.send(Message::Ping(b"starfruit".to_vec()));
            }
            frame = incoming.next() => {
                match frame {
                    None => break Err("连接已关闭".into()),
                    Some(Err(e)) => break Err(format!("读取失败: {e}")),
                    Some(Ok(Message::Close(_))) => break Err("服务端关闭连接".into()),
                    Some(Ok(Message::Text(text))) => {
                        if let Some(err) = on_frame(cfg, &inbox_path, &text) {
                            break Err(err);
                        }
                    }
                    _ => {}
                }
            }
        }
    };

    outbox_task.abort();
    writer.abort();
    result
}

async fn connect_once(cfg: &Cfg) -> Result<(), String> {
    let url = cfg
        .server
        .strip_prefix("wss://")
        .map(|r| (r, true))
        .or_else(|| cfg.server.strip_prefix("ws://").map(|r| (r, false)))
        .ok_or("server 必须以 ws:// 或 wss:// 开头")?;
    let (rest, use_tls) = url;
    let default_port = if use_tls { 443 } else { 80 };
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => {
            (h.to_string(), p.parse().unwrap_or(default_port))
        }
        _ => (rest.to_string(), default_port),
    };

    log(&format!("连接 {}（mode={}）", cfg.server, cfg.mode));
    let raw = TcpStream::connect((host.as_str(), port))
        .await
        .map_err(|e| format!("TCP 连接失败: {e}"))?;

    if use_tls {
        let config = build_tls_config(cfg.ca_cert.as_ref())?;
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
        let server_name =
            rustls::pki_types::ServerName::try_from(host.clone()).map_err(|e| e.to_string())?;
        let tls_stream = connector.connect(server_name, raw).await.map_err(|e| {
            format!("TLS 握手失败（服务端可能运行在非加密模式 --mode plain）: {e}")
        })?;
        let (ws, _resp) = tokio_tungstenite::client_async(cfg.server.as_str(), tls_stream)
            .await
            .map_err(map_ws_err)?;
        return run_session(cfg, ws).await;
    }
    let (ws, _resp) = tokio_tungstenite::client_async(cfg.server.as_str(), raw)
        .await
        .map_err(map_ws_err)?;
    run_session(cfg, ws).await
}

/// WS 握手错误分类：服务端加密方式不匹配时会回明文 STARFRUIT_ERR 行（随 HTTP 响应体带出）。
fn map_ws_err(e: tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error as E;
    match e {
        E::Http(resp) => {
            let body = resp
                .body()
                .as_ref()
                .map(|b| String::from_utf8_lossy(b).trim().to_string())
                .unwrap_or_default();
            if body.contains("STARFRUIT_ERR") {
                format!("主节点拒绝连接：{body}")
            } else {
                format!("WS 握手被拒绝：{body}")
            }
        }
        other => format!("WS 握手失败: {other}"),
    }
}

/// 处理一帧；返回 `Some(err)` 表示应终止连接（致命）。
fn on_frame(cfg: &Cfg, inbox_path: &std::path::Path, text: &str) -> Option<String> {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => {
            log(&format!("收到非 JSON 帧: {:.80}", text));
            return None;
        }
    };
    if v.get("system").and_then(|s| s.as_bool()).unwrap_or(false) {
        let code = v.get("code").and_then(|c| c.as_str()).unwrap_or("");
        if code == "mode_mismatch" {
            log(&format!(
                "加密模式不匹配（服务端报错）：{}",
                v.get("detail").and_then(|d| d.as_str()).unwrap_or("")
            ));
            return Some("mode_mismatch".into());
        }
        log(&format!(
            "系统回执 {code}: {}",
            v.get("detail").and_then(|d| d.as_str()).unwrap_or("")
        ));
        return None;
    }
    append_line(inbox_path, text);
    let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    log(&format!(
        "收到 {} → {} [{}] {:.60}",
        v.get("from").and_then(|f| f.as_str()).unwrap_or(""),
        v.get("to").and_then(|t| t.as_str()).unwrap_or(""),
        kind,
        v.get("text").and_then(|t| t.as_str()).unwrap_or("")
    ));
    if kind == "wake" {
        trigger_wake(cfg, &v);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 时间戳格式化_为_utc_展示形状() {
        // 2026-11-12 09:00:00 UTC = 1794474000000 ms
        assert_eq!(fmt_ts(1794474000000), "2026-11-12 09:00:00");
    }
}
