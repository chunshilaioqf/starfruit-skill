//! starfruit-master：主节点（管理者）入口。
//!
//! 星型拓扑 hub + 只读 Web 面板 + 内置管理者端点（inbox/outbox/wake）。
//! 全部 Rust 实现；由用户本地编译（不提供二进制）。
//! 用法见 `--help` 与 SKILL.md。

mod agent;
mod auth;
mod hub;
mod protocol;
mod tls;
mod web;

use std::path::PathBuf;
use std::sync::Arc;

use auth::TokenAuth;
use protocol::{Deploy, Mode, State};

struct Config {
    deploy: Deploy,
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
    jwt_secret: Option<String>,
    jwt_ttl: u64,
    require_jwt: bool,
    issue_token: Option<String>,
    daemon: bool,
    daemon_child: bool,
}

/// 所有模式允许的最小监听端口（默认端口为 10001，被占用则 +1）。
const MIN_PORT: u16 = 10001;

const HELP: &str = r#"starfruit-master —— 多节点 WS 协同主节点（管理者，全网唯一）

用法: starfruit-master [选项]

选项:
  --deploy local|lan|public  部署模式（默认 local）：
                               local  本机模式：只监听 127.0.0.1；tls / jwt 可选
                               lan    局域网模式：监听本机局域网 IP；tls / jwt 可选
                               public 公网模式：必须指定具体 IP（禁止 0.0.0.0），
                                      必须 --mode tls，且强制启用 JWT 令牌校验
  --mode plain|tls      运行模式：非加密(ws)/加密(wss)（默认 plain）
  --hub-addr <addr>     WS 消息枢纽监听地址（默认 127.0.0.1:10001，禁止 0.0.0.0）
  --web-addr <addr>     Web 只读面板监听地址（默认 127.0.0.1:10002，禁止 0.0.0.0）
  --cert <path>         TLS 证书 PEM（缺省在 data-dir 自动生成自签证书）
  --key <path>          TLS 私钥 PEM（与 --cert 同时提供）
  --jwt-secret <s>      JWT 签名密钥（缺省自动生成 data-dir/jwt-secret，0600 落盘）
  --jwt-ttl <secs>      JWT 令牌有效期秒数（默认 86400）
  --require-jwt         local/lan 模式下手动启用 JWT 令牌校验（公网模式强制启用）
  --issue-token <name>  为指定节点签发 JWT 令牌并打印后退出（不启动服务）
  --data-dir <dir>      运行数据目录（日志/pid/历史/wake，默认 ./starfruit-data）
  --manager-name <name> 内置管理者端点的注册名（默认 manager）
  --wake-cmd <shell>    wake 唤醒钩子：命令（环境变量 SF_FROM/SF_TO/SF_TEXT/SF_TS）
  --wake-url <url>      wake 唤醒钩子：HTTP POST JSON
  --history-cap <n>     面板内存历史条数（默认 5000，JSONL 落盘不限）
  --daemon              后台运行（脱离终端，日志写入 data-dir/starfruit-master.log）
  -h, --help            显示帮助

监听与安全规则（务必遵守）:
  · 端口规则：所有模式默认 10001（面板 10002），且必须 ≥ 10001；被占用则自动 +1。
  · 本机模式(local)  ：只监听 127.0.0.1（默认，最安全）；tls / jwt 可选。
  · 局域网模式(lan)  ：监听本机局域网 IP（如 192.168.x.x），禁止监听 0.0.0.0；tls / jwt 可选。
  · 公网模式(public) ：必须指定具体 IP（禁止 0.0.0.0），必须 --mode tls 且强制 jwt；
                       配合 Cloudflare Tunnel 服务时监听 127.0.0.1 作为隧道指向即可。

JWT 令牌（公网模式强制，local/lan 可选）: 令牌只能由主节点签发，所有子节点必须携带校验。
  · 签发：starfruit-master --issue-token <节点名> [--jwt-secret <s>] [--jwt-ttl <secs>]
  · 使用：子节点以 --token <令牌> 启动（Rust/Python/Node.js 三种客户端均支持）

模式不匹配（对方加密方式不对）时会向对方报错后断开（见 docs/protocol.md §2.1）。
"#;

/// 取监听地址的 host 部分（兼容 `[IPv6]:port` 写法）。
fn host_of(addr: &str) -> &str {
    if let Some(rest) = addr.strip_prefix('[') {
        if let Some(idx) = rest.find(']') {
            return &rest[..idx];
        }
    }
    addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr)
}

/// 是否为"监听全部网卡"的通配地址（被明确禁止）。
fn is_wildcard_host(host: &str) -> bool {
    matches!(host, "0.0.0.0" | "::" | "0:0:0:0:0:0:0:0")
}

/// 是否为仅本机可访问的回环地址。
fn is_loopback_host(host: &str) -> bool {
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

/// 拆分监听地址为 (host, port)。
fn split_host_port(addr: &str) -> Option<(String, u16)> {
    let (h, p) = addr.rsplit_once(':')?;
    let port: u16 = p.trim().trim_end_matches(']').parse().ok()?;
    Some((h.trim_start_matches('[').to_string(), port))
}

/// 组合 (host, port) 为监听地址（IPv6 自动加方括号）。
fn join_host_port(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// 校验监听地址：禁止通配地址（0.0.0.0 / ::），并要求端口 ≥ MIN_PORT。
fn validate_bind(label: &str, addr: &str) {
    if is_wildcard_host(host_of(addr)) {
        eprintln!("[fatal] {label} 不允许监听通配地址 {addr}。");
        eprintln!("        仅本机使用请用 127.0.0.1；需局域网请用本机局域网 IP（如 192.168.x.x）；");
        eprintln!("        需公网请用 Cloudflare Tunnel 暴露并配合 --mode tls + JWT 令牌。详见 --help。");
        std::process::exit(1);
    }
    match auth::port_of(addr) {
        Some(p) if p >= MIN_PORT => {}
        Some(p) => {
            eprintln!("[fatal] {label} 端口 {p} 不合法：所有模式启动端口必须 ≥ {MIN_PORT}（默认 10001）。");
            std::process::exit(1);
        }
        None => {
            eprintln!("[fatal] {label} 缺少端口：格式应为 <host>:<port>（如 127.0.0.1:10001）。");
            std::process::exit(1);
        }
    }
}

/// 部署模式校验：监听地址范围（local 限回环 / lan 需局域网 IP / public 须具体 IP）+ tls 要求。
fn validate_deploy(cfg: &Config) {
    let hub_host = host_of(&cfg.hub_addr).to_string();
    match cfg.deploy {
        Deploy::Local => {
            if !is_loopback_host(&hub_host) {
                eprintln!(
                    "[fatal] 本机模式(--deploy local)只允许监听回环地址（127.0.0.1），当前为 {}。",
                    cfg.hub_addr
                );
                eprintln!("        需局域网使用请用 --deploy lan 并监听本机局域网 IP（如 192.168.x.x）；");
                eprintln!("        需公网支持请用 --deploy public（必须 --mode tls 且强制 jwt）。");
                std::process::exit(1);
            }
        }
        Deploy::Lan => {
            if is_loopback_host(&hub_host) {
                eprintln!(
                    "[fatal] 局域网模式(--deploy lan)需监听本机局域网 IP（如 192.168.x.x），当前为 {}。",
                    cfg.hub_addr
                );
                eprintln!("        仅本机多 agent 工具使用请用 --deploy local（监听 127.0.0.1，默认）。");
                std::process::exit(1);
            }
        }
        Deploy::Public => {
            // 地址必须为具体 IP（通配地址已在 validate_bind 拒绝）；tls 必选。
            if cfg.mode != Mode::Tls {
                eprintln!("[fatal] 公网模式(--deploy public)必须启用加密：请加 --mode tls。");
                eprintln!("        配合 Cloudflare Tunnel 时监听 127.0.0.1 作为隧道指向即可。");
                std::process::exit(1);
            }
        }
    }
}

/// 是否强制启用 JWT 令牌校验（公网模式强制；local/lan 需显式 --require-jwt）。
fn jwt_required(cfg: &Config) -> bool {
    cfg.require_jwt || cfg.deploy == Deploy::Public
}

/// 绑定监听地址：端口被占用则自动 +1 重试（上限 100 次）。
async fn bind_with_retry(addr: &str) -> Result<(tokio::net::TcpListener, String), String> {
    let (host, mut port) =
        split_host_port(addr).ok_or_else(|| format!("无法解析监听地址 {addr}"))?;
    for _ in 0..100 {
        let candidate = join_host_port(&host, port);
        match tokio::net::TcpListener::bind(&candidate).await {
            Ok(l) => return Ok((l, candidate)),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                port = port.saturating_add(1);
            }
            Err(e) => return Err(format!("绑定 {candidate} 失败: {e}")),
        }
    }
    Err(format!("绑定 {addr} 失败：连续 100 个端口均被占用"))
}

/// 启动成功后打印的会话说明：主节点信息 + 子节点加入方式 + 部署/监听规则。
fn print_banner(cfg: &Config, hub_addr: &str, web_addr: &str, jwt_required: bool) {
    let scheme = if cfg.mode == Mode::Tls { "wss" } else { "ws" };
    let web_scheme = if cfg.mode == Mode::Tls { "https" } else { "http" };
    let mode = cfg.mode.as_str();
    let mode_desc = if cfg.mode == Mode::Tls { "wss 加密" } else { "ws 明文" };
    let server = format!("{scheme}://{hub_addr}");
    let token_hint = if jwt_required { " --token <令牌>" } else { "" };
    let jwt_desc = if jwt_required {
        "已强制启用（令牌只能由主节点签发，子节点必须携带 --token）"
    } else {
        "未启用（local/lan 可选：--require-jwt 开启；公网模式强制）"
    };
    let sep = "────────────────────────────────────────────────────────────────────";
    eprintln!("{sep}");
    eprintln!("⭐ starfruit-master 已启动（主节点 · 管理者 · 全网唯一）");
    eprintln!();
    eprintln!("  主节点名称 : {}", cfg.manager_name);
    eprintln!("  部署模式   : {}（{}）", cfg.deploy.as_str(), cfg.deploy.desc());
    eprintln!("  运行模式   : {mode}（{mode_desc}）");
    eprintln!("  JWT 校验   : {jwt_desc}");
    eprintln!("  消息枢纽   : {server}");
    eprintln!("  只读面板   : {web_scheme}://{web_addr}");
    eprintln!("  数据目录   : {}", cfg.data_dir.display());
    eprintln!();
    eprintln!("  其它执行节点加入方式（任选一种客户端，功能等价）：");
    eprintln!("    Rust    : starfruit-node    --name <唯一名> --mode {mode} --server {server}{token_hint}");
    eprintln!("    Python3 : starfruit_node.py --name <唯一名> --mode {mode} --server {server}{token_hint}");
    eprintln!("    Node.js : starfruit-node.js --name <唯一名> --mode {mode} --server {server}{token_hint}");
    eprintln!("  · 客户端用哪种实现由 agent 按所在机器环境决定：有 Rust 工具链用 Rust 版；");
    eprintln!("    仅有 Python 用 Python 版；仅有 Node 用 Node.js 版。三者功能等价。");
    eprintln!("  · --name 全网唯一（同名重连顶掉旧连接）；--mode 必须与主节点一致，否则被报错断开。");
    if jwt_required {
        eprintln!("  · 本主节点已强制 JWT 校验：先执行 `starfruit-master --issue-token <节点名> --data-dir {}`", cfg.data_dir.display());
        eprintln!("    签发令牌，子节点启动时须加 --token <令牌>，否则将被拒绝（unauthorized）。");
    }
    eprintln!();
    eprintln!("  部署与监听规则（务必遵守）：");
    eprintln!("  · 端口：所有模式默认 10001（面板 10002），必须 ≥ 10001；被占用自动 +1。");
    eprintln!("  · 本机模式(local)  ：只监听 127.0.0.1（默认，最安全）；tls / jwt 可选。");
    eprintln!("  · 局域网模式(lan)  ：监听本机局域网 IP（如 192.168.x.x），禁止监听 0.0.0.0；tls / jwt 可选。");
    eprintln!("  · 公网模式(public) ：必须指定具体 IP（禁止 0.0.0.0），必须 --mode tls 且强制 jwt；");
    eprintln!("    配合 Cloudflare Tunnel 时监听 127.0.0.1 作为隧道指向即可。");
    eprintln!("{sep}");
}

fn parse_args() -> Config {
    let mut cfg = Config {
        deploy: Deploy::Local,
        mode: Mode::Plain,
        hub_addr: "127.0.0.1:10001".into(),
        web_addr: "127.0.0.1:10002".into(),
        cert: None,
        key: None,
        data_dir: PathBuf::from("starfruit-data"),
        manager_name: "manager".into(),
        wake_cmd: None,
        wake_url: None,
        history_cap: 5000,
        jwt_secret: None,
        jwt_ttl: auth::DEFAULT_TTL_SECS,
        require_jwt: false,
        issue_token: None,
        daemon: false,
        daemon_child: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = |name: &str| {
            args.next().unwrap_or_else(|| panic!("{name} 缺少参数值"))
        };
        match a.as_str() {
            "--deploy" => {
                let v = val("--deploy");
                cfg.deploy = Deploy::parse(&v).unwrap_or_else(|| {
                    panic!("--deploy 只能是 local|lan|public，收到 {v}")
                });
            }
            "--mode" => {
                let v = val("--mode");
                cfg.mode = Mode::parse(&v).unwrap_or_else(|| panic!("--mode 只能是 plain|tls，收到 {v}"));
            }
            "--hub-addr" => cfg.hub_addr = val("--hub-addr"),
            "--web-addr" => cfg.web_addr = val("--web-addr"),
            "--cert" => cfg.cert = Some(PathBuf::from(val("--cert"))),
            "--key" => cfg.key = Some(PathBuf::from(val("--key"))),
            "--jwt-secret" => cfg.jwt_secret = Some(val("--jwt-secret")),
            "--jwt-ttl" => {
                cfg.jwt_ttl = val("--jwt-ttl").parse().expect("--jwt-ttl 需要整数秒")
            }
            "--require-jwt" => cfg.require_jwt = true,
            "--issue-token" => cfg.issue_token = Some(val("--issue-token")),
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
    // 监听规则：禁止通配地址（0.0.0.0 / ::）+ 端口下限（≥ 10001）。
    validate_bind("--hub-addr", &cfg.hub_addr);
    validate_bind("--web-addr", &cfg.web_addr);
    // 部署模式规则：地址范围 + tls 要求。
    validate_deploy(&cfg);
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

    // JWT 校验器：公网模式强制；local/lan 需 --require-jwt；--issue-token 亦需构造（签发用）。
    let need_jwt = jwt_required(&cfg) || cfg.issue_token.is_some();
    let auth = if need_jwt {
        match TokenAuth::load_or_create(&cfg.data_dir, cfg.jwt_secret.as_deref(), cfg.jwt_ttl) {
            Ok(a) => Some(Arc::new(a)),
            Err(e) => {
                eprintln!("[fatal] JWT 初始化失败: {e}");
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    // --issue-token：签发令牌后打印并退出（不启动服务）。
    if let Some(name) = cfg.issue_token.clone() {
        let a = auth.as_ref().expect("issue-token 需要 JWT 校验器");
        println!("{}", a.issue(&name));
        eprintln!(
            "[jwt] 已为节点 {name} 签发令牌（有效期 {} 秒，仅本主节点认可）",
            a.ttl()
        );
        return;
    }

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
    runtime.block_on(run(cfg, state, acceptor, history_records, seq, auth));
}

async fn run(
    cfg: Config,
    state: Arc<State>,
    acceptor: Option<tokio_rustls::TlsAcceptor>,
    preload: std::collections::VecDeque<serde_json::Value>,
    preload_seq: u64,
    auth: Option<Arc<TokenAuth>>,
) {
    // 预载历史。
    {
        let mut hist = state.history.lock().await;
        hist.records = preload;
        hist.seq = preload_seq;
    }

    let jwt = jwt_required(&cfg);
    if cfg.deploy == Deploy::Public {
        eprintln!("[强制] 公网模式：已强制启用 JWT 令牌校验（tls + jwt 均为必选）。");
        eprintln!("       请用 `starfruit-master --issue-token <节点名> --data-dir {}` 为每个子节点签发令牌，", cfg.data_dir.display());
        eprintln!("       子节点以 --token <令牌> 连接；未携带或校验失败的连接会被拒绝（unauthorized）。");
    }

    eprintln!(
        "[starfruit-master] 部署={} 模式={} hub={} web={} 数据目录={}",
        cfg.deploy.as_str(),
        cfg.mode.as_str(),
        cfg.hub_addr,
        cfg.web_addr,
        cfg.data_dir.display()
    );

    // Web 只读面板（端口占用自动 +1）。
    let (web_listener, web_addr) = match bind_with_retry(&cfg.web_addr).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[fatal] web {e}");
            std::process::exit(1);
        }
    };
    tokio::spawn(web::serve(
        state.clone(),
        web_listener,
        web_addr.clone(),
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

    // WS 消息枢纽（端口占用自动 +1）。
    let (listener, hub_addr) = match bind_with_retry(&cfg.hub_addr).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[fatal] hub {e}");
            std::process::exit(1);
        }
    };
    let mode = cfg.mode;
    eprintln!(
        "[hub] 消息枢纽监听 {}://{}",
        if mode == Mode::Tls { "wss" } else { "ws" },
        hub_addr
    );

    // 明文监听非本机地址时给出提示：公网务必用 Cloudflare Tunnel + tls + jwt。
    if mode == Mode::Plain && !is_loopback_host(host_of(&hub_addr)) {
        eprintln!(
            "[warn] 当前以明文(plain)监听非本机地址 {}：仅建议可信局域网使用；\
公网请改用 Cloudflare Tunnel 暴露并配合 --mode tls + JWT 令牌。",
            hub_addr
        );
    }

    // 启动成功说明（会话提示）。
    print_banner(&cfg, &hub_addr, &web_addr, jwt);

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, peer_addr)) => {
                        let state = state.clone();
                        let acceptor = acceptor.clone();
                        let auth = auth.clone();
                        tokio::spawn(async move {
                            if let Err(e) = hub::serve(stream, state, mode, acceptor, auth).await {
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
