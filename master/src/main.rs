//! starfruit-master：主节点（管理者）入口。
//!
//! 星型拓扑 hub + 只读 Web 面板 + 内置管理者端点（inbox/outbox/wake）。
//! 全部 Rust 实现；由用户本地编译（不提供二进制）。
//! 用法见 `--help` 与 SKILL.md。

mod agent;
mod hub;
mod protocol;
mod tls;
mod web;

use std::path::PathBuf;
use std::sync::Arc;

use protocol::{Mode, State};

struct Config {
    mode: Mode,
    hub_addr: String,
    web_addr: String,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    data_dir: PathBuf,
    manager_name: String,
    wake_cmd: Option<String>,
    wake_url: Option<String>,
    history_cap: usize,
    daemon: bool,
    daemon_child: bool,
}

const HELP: &str = r#"starfruit-master —— 多节点 WS 协同主节点（管理者，全网唯一）

用法: starfruit-master [选项]

选项:
  --mode plain|tls      运行模式：非加密(ws)/加密(wss)（默认 plain）
  --hub-addr <addr>     WS 消息枢纽监听地址（默认 0.0.0.0:9501）
  --web-addr <addr>     Web 只读面板监听地址（默认 127.0.0.1:9502）
  --cert <path>         TLS 证书 PEM（缺省在 data-dir 自动生成自签证书）
  --key <path>          TLS 私钥 PEM（与 --cert 同时提供）
  --data-dir <dir>      运行数据目录（日志/pid/历史/wake，默认 ./starfruit-data）
  --manager-name <name> 内置管理者端点的注册名（默认 manager）
  --wake-cmd <shell>    wake 唤醒钩子：命令（环境变量 SF_FROM/SF_TO/SF_TEXT/SF_TS）
  --wake-url <url>      wake 唤醒钩子：HTTP POST JSON
  --history-cap <n>     面板内存历史条数（默认 5000，JSONL 落盘不限）
  --daemon              后台运行（脱离终端，日志写入 data-dir/starfruit-master.log）
  -h, --help            显示帮助

模式不匹配（对方加密方式不对）时会向对方报错后断开（见 docs/protocol.md §2.1）。
"#;

fn parse_args() -> Config {
    let mut cfg = Config {
        mode: Mode::Plain,
        hub_addr: "0.0.0.0:9501".into(),
        web_addr: "127.0.0.1:9502".into(),
        cert: None,
        key: None,
        data_dir: PathBuf::from("starfruit-data"),
        manager_name: "manager".into(),
        wake_cmd: None,
        wake_url: None,
        history_cap: 5000,
        daemon: false,
        daemon_child: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = |name: &str| {
            args.next().unwrap_or_else(|| panic!("{name} 缺少参数值"))
        };
        match a.as_str() {
            "--mode" => {
                let v = val("--mode");
                cfg.mode = Mode::parse(&v).unwrap_or_else(|| panic!("--mode 只能是 plain|tls，收到 {v}"));
            }
            "--hub-addr" => cfg.hub_addr = val("--hub-addr"),
            "--web-addr" => cfg.web_addr = val("--web-addr"),
            "--cert" => cfg.cert = Some(PathBuf::from(val("--cert"))),
            "--key" => cfg.key = Some(PathBuf::from(val("--key"))),
            "--data-dir" => cfg.data_dir = PathBuf::from(val("--data-dir")),
            "--manager-name" => cfg.manager_name = val("--manager-name"),
            "--wake-cmd" => cfg.wake_cmd = Some(val("--wake-cmd")),
            "--wake-url" => cfg.wake_url = Some(val("--wake-url")),
            "--history-cap" => {
                cfg.history_cap = val("--history-cap").parse().expect("--history-cap 需要整数")
            }
            "--daemon" => cfg.daemon = true,
            "--_daemon-child" => cfg.daemon_child = true,
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            other => panic!("未知参数: {other}（--help 查看用法）"),
        }
    }
    cfg
}

/// 后台化：以 setsid 方式重新拉起自身，stdio 重定向到日志文件，父进程退出。
fn daemonize(cfg: &Config) {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().expect("无法定位自身可执行文件");
    let log = cfg.data_dir.join("starfruit-master.log");
    std::fs::create_dir_all(&cfg.data_dir).expect("创建 data-dir 失败");
    let out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .expect("打开日志文件失败");
    let err = out.try_clone().expect("克隆日志句柄失败");
    let mut cmd = std::process::Command::new(exe);
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
    println!("starfruit-master 已后台启动（pid {}），日志: {}", child.id(), log.display());
    std::process::exit(0);
}

fn main() {
    let cfg = parse_args();
    std::fs::create_dir_all(&cfg.data_dir).expect("创建 data-dir 失败");
    if cfg.daemon && !cfg.daemon_child {
        daemonize(&cfg);
    }

    // pidfile
    let pid_file = cfg.data_dir.join("starfruit-master.pid");
    let _ = std::fs::write(&pid_file, std::process::id().to_string());

    // 历史：JSONL 追加 + 预载最近记录（面板跨重启可回看）。
    use std::io::{BufRead, BufReader};
    let history_path = cfg.data_dir.join("history.jsonl");
    let mut history_records = std::collections::VecDeque::new();
    let mut seq: u64 = 0;
    if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).read(true).open(&history_path) {
        let reader = BufReader::new(f.try_clone().expect("克隆历史文件失败"));
        for line in reader.lines().map_while(Result::ok) {
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&line) {
                seq = v["seq"].as_u64().unwrap_or(seq);
                v["seq"] = serde_json::Value::from(seq);
                history_records.push_back(v);
                while history_records.len() > cfg.history_cap {
                    history_records.pop_front();
                }
            }
        }
        let _ = f;
    }
    let history_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&history_path)
        .ok();
    let state = State::new(cfg.history_cap, history_file);

    // TLS（如需）。
    let acceptor = if cfg.mode == Mode::Tls {
        match tls::build_acceptor(cfg.cert.as_deref(), cfg.key.as_deref(), &cfg.data_dir) {
            Ok(a) => Some(a),
            Err(e) => {
                eprintln!("[fatal] TLS 初始化失败: {e}");
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("构建 tokio runtime 失败");
    runtime.block_on(run(cfg, state, acceptor, history_records, seq));
}

async fn run(
    cfg: Config,
    state: Arc<State>,
    acceptor: Option<tokio_rustls::TlsAcceptor>,
    preload: std::collections::VecDeque<serde_json::Value>,
    preload_seq: u64,
) {
    // 预载历史。
    {
        let mut hist = state.history.lock().await;
        hist.records = preload;
        hist.seq = preload_seq;
    }

    eprintln!(
        "[starfruit-master] 模式={} hub={} web={} 数据目录={}",
        cfg.mode.as_str(),
        cfg.hub_addr,
        cfg.web_addr,
        cfg.data_dir.display()
    );

    // Web 只读面板。
    tokio::spawn(web::serve(
        state.clone(),
        cfg.web_addr.clone(),
        cfg.mode,
        acceptor.clone(),
    ));

    // 内置管理者端点（inbox/outbox/wake）。
    tokio::spawn(agent::run(
        state.clone(),
        agent::AgentCfg {
            name: cfg.manager_name.clone(),
            data_dir: cfg.data_dir.clone(),
            wake_cmd: cfg.wake_cmd.clone(),
            wake_url: cfg.wake_url.clone(),
        },
    ));

    // WS 消息枢纽。
    let listener = tokio::net::TcpListener::bind(&cfg.hub_addr)
        .await
        .unwrap_or_else(|e| panic!("hub 无法绑定 {}: {e}", cfg.hub_addr));
    let mode = cfg.mode;
    eprintln!(
        "[hub] 消息枢纽监听 {}://{}",
        if mode == Mode::Tls { "wss" } else { "ws" },
        cfg.hub_addr
    );

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, peer_addr)) => {
                        let state = state.clone();
                        let acceptor = acceptor.clone();
                        tokio::spawn(async move {
                            if let Err(e) = hub::serve(stream, state, mode, acceptor).await {
                                eprintln!("[hub] {peer_addr} 连接结束: {e}");
                            }
                        });
                    }
                    Err(e) => eprintln!("[hub] accept 失败: {e}"),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("[starfruit-master] 收到 Ctrl-C，退出");
                let _ = std::fs::remove_file(cfg.data_dir.join("starfruit-master.pid"));
                break;
            }
        }
    }
}
