//! starfruit-master：主节点（管理者）入口。
//!
//! 星型拓扑 hub + 只读 Web 面板 + 内置管理者端点（inbox/outbox/wake）。
//! 全部 Rust 实现；由用户本地编译（不提供二进制）。
//! 用法见 `--help` 与 SKILL.md。

mod agent;
mod auth;
mod hub;
mod nodesdoc;
mod paths;
mod protocol;
mod setup;
mod team;
mod tls;
mod update;
mod web;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use auth::TokenAuth;
use protocol::{Deploy, Mode, State};
use team::{JoinCtx, Team};

/// 记录哪些参数来自命令行（setup.json 只回填未显式指定的项）。
#[derive(Default)]
struct Explicit {
    deploy: bool,
    mode: bool,
    hub_addr: bool,
    web_addr: bool,
    jwt_ttl: bool,
    require_jwt: bool,
    team: bool,
    nodes_doc: bool,
}

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
    team: Option<PathBuf>,
    init_team: Option<PathBuf>,
    nodes_doc: Option<String>,
    tunnel_url: Option<String>,
    setup: bool,
    no_setup: bool,
    daemon: bool,
    daemon_child: bool,
    explicit: Explicit,
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
  --team <team.json>    加载工作团队定义：为主节点生成每位成员"加入团队"的完整提示词
                        （终端可复制 + 落盘 data-dir/team-prompts.md + 面板 /team 查看）
  --init-team <path>    写出工作团队配置模板到 <path> 并退出（用户填写后用 --team 加载）
  --setup               强制进入首次启动向导（部署模式/团队/令牌有效期交互选择）
  --no-setup            跳过首次启动向导（脚本化启动用）
  --nodes-doc <path>    starfruit-nodes.md 保存路径（缺省自动：docs/ > doc/ > 当前目录）
  --tunnel-url <url>    公网 Cloudflare Tunnel 地址（缺省读 ~/.starfruit/tunnel/url，
                        由 scripts/starfruit-tunnel.sh 独立维护，主节点不随其重启）
  --data-dir <dir>      运行数据目录（默认 ~/.starfruit/master，用户级存储，跨平台一致）
  --manager-name <name> 内置管理者端点的注册名（默认 manager）
  --wake-cmd <shell>    wake 唤醒钩子：命令（环境变量 SF_FROM/SF_TO/SF_TEXT/SF_TS）
  --wake-url <url>      wake 唤醒钩子：HTTP POST JSON
  --history-cap <n>     面板内存历史条数（默认 5000，JSONL 落盘不限）
  --daemon              后台运行（脱离终端，日志写入 data-dir/starfruit-master.log）
  -h, --help            显示帮助

首次启动: data-dir 无 setup.json 且 stdin 为终端时自动进入向导：
  选择部署模式 → 创建工作团队（名称唯一）→ 选择令牌有效期 → 签发令牌并启动；
  会话输出地址端口/团队成员/令牌，并生成 starfruit-nodes.md（自动加入 .gitignore，
  节点信息变动时自动更新）。选择持久化 setup.json，之后启动自动复用。

监听与安全规则（务必遵守）:
  · 端口规则：所有模式默认 10001（面板 10002），且必须 ≥ 10001；被占用则自动 +1。
  · 本机模式(local)  ：只监听 127.0.0.1（默认，最安全）；tls / jwt 可选。
  · 局域网模式(lan)  ：监听本机局域网 IP（如 192.168.x.x），禁止监听 0.0.0.0；tls / jwt 可选。
  · 公网模式(public) ：必须指定具体 IP（禁止 0.0.0.0），必须 --mode tls 且强制 jwt；
                       配合 Cloudflare Tunnel 服务时监听 127.0.0.1 作为隧道指向即可。
  · 节点名称全网唯一：创建团队与节点登录均校验，重名在线连接被拒绝（name_conflict）。

JWT 令牌（公网模式强制，local/lan 可选）: 令牌只能由主节点签发，所有子节点必须携带校验。
  · 签发：starfruit-master --issue-token <节点名> [--jwt-secret <s>] [--jwt-ttl <secs>]
  · 使用：子节点以 --token <令牌> 启动（Rust/Python/Node.js 三种客户端均支持）

更新检查: 启动时每周最多一次比对线上 CHANGELOG.md（仓库
  https://github.com/chunshilaioqf/starfruit-skill）；有更新会提醒，
  更新用 scripts/starfruit-update.sh（含数据迁移检查）。

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

/// 组装团队提示词上下文（真实 hub 地址 / 模式 / 是否需令牌 / 数据目录）。
fn join_ctx(cfg: &Config, mode: Mode, hub_addr: &str, jwt_required: bool) -> JoinCtx {
    let scheme = if mode == Mode::Tls { "wss" } else { "ws" };
    JoinCtx {
        server: format!("{scheme}://{hub_addr}"),
        mode: mode.as_str().to_string(),
        token_required: jwt_required,
        data_dir: cfg.data_dir.display().to_string(),
    }
}

/// 启动成功后打印的会话说明：主节点信息 + 子节点加入方式 + 部署/监听规则。
fn print_banner(
    cfg: &Config,
    hub_addr: &str,
    web_addr: &str,
    jwt_required: bool,
    tunnel: Option<&str>,
    doc_path: &std::path::Path,
    tokens: &HashMap<String, String>,
) {
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
    if let Some(url) = tunnel {
        eprintln!("  公网地址   : {url}（Cloudflare Tunnel 独立后台运行，主节点重启不影响）");
    }
    eprintln!("  数据目录   : {}", cfg.data_dir.display());
    eprintln!("  节点文档   : {}（节点信息变动时自动更新）", doc_path.display());
    eprintln!("  项目仓库   : {}", update::REPO_URL);
    if !tokens.is_empty() {
        eprintln!();
        eprintln!("  团队成员令牌（本次启动签发，有效期 {} 秒；已写入节点文档与 tokens.json）：", cfg.jwt_ttl);
        let mut names: Vec<&String> = tokens.keys().collect();
        names.sort();
        for name in names {
            eprintln!("    {name} : {}", tokens[name]);
        }
    }
    eprintln!();
    eprintln!("  其它执行节点加入方式（任选一种客户端，功能等价）：");
    eprintln!("    Rust    : starfruit-node    --name <唯一名> --mode {mode} --server {server}{token_hint}");
    eprintln!("    Python3 : starfruit_node.py --name <唯一名> --mode {mode} --server {server}{token_hint}");
    eprintln!("    Node.js : starfruit-node.js --name <唯一名> --mode {mode} --server {server}{token_hint}");
    eprintln!("  · 客户端用哪种实现由 agent 按所在机器环境决定：有 Rust 工具链用 Rust 版；");
    eprintln!("    仅有 Python 用 Python 版；仅有 Node 用 Node.js 版。三者功能等价。");
    eprintln!("  · --name 全网唯一（重名在线连接被拒绝 name_conflict）；--mode 必须与主节点一致。");
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
        data_dir: paths::default_master_data_dir(),
        manager_name: "manager".into(),
        wake_cmd: None,
        wake_url: None,
        history_cap: 5000,
        jwt_secret: None,
        jwt_ttl: auth::DEFAULT_TTL_SECS,
        require_jwt: false,
        issue_token: None,
        team: None,
        init_team: None,
        nodes_doc: None,
        tunnel_url: None,
        setup: false,
        no_setup: false,
        daemon: false,
        daemon_child: false,
        explicit: Explicit::default(),
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
                cfg.explicit.deploy = true;
            }
            "--mode" => {
                let v = val("--mode");
                cfg.mode = Mode::parse(&v).unwrap_or_else(|| panic!("--mode 只能是 plain|tls，收到 {v}"));
                cfg.explicit.mode = true;
            }
            "--hub-addr" => {
                cfg.hub_addr = val("--hub-addr");
                cfg.explicit.hub_addr = true;
            }
            "--web-addr" => {
                cfg.web_addr = val("--web-addr");
                cfg.explicit.web_addr = true;
            }
            "--cert" => cfg.cert = Some(PathBuf::from(val("--cert"))),
            "--key" => cfg.key = Some(PathBuf::from(val("--key"))),
            "--jwt-secret" => cfg.jwt_secret = Some(val("--jwt-secret")),
            "--jwt-ttl" => {
                cfg.jwt_ttl = val("--jwt-ttl").parse().expect("--jwt-ttl 需要整数秒");
                cfg.explicit.jwt_ttl = true;
            }
            "--require-jwt" => {
                cfg.require_jwt = true;
                cfg.explicit.require_jwt = true;
            }
            "--issue-token" => cfg.issue_token = Some(val("--issue-token")),
            "--team" => {
                cfg.team = Some(PathBuf::from(val("--team")));
                cfg.explicit.team = true;
            }
            "--init-team" => cfg.init_team = Some(PathBuf::from(val("--init-team"))),
            "--nodes-doc" => {
                cfg.nodes_doc = Some(val("--nodes-doc"));
                cfg.explicit.nodes_doc = true;
            }
            "--tunnel-url" => cfg.tunnel_url = Some(val("--tunnel-url")),
            "--setup" => cfg.setup = true,
            "--no-setup" => cfg.no_setup = true,
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

/// 用 setup.json 的选择回填未显式指定的命令行参数。
fn apply_setup(cfg: &mut Config, s: &setup::Setup) {
    if !cfg.explicit.deploy {
        cfg.deploy = Deploy::parse(&s.deploy).unwrap_or(Deploy::Local);
    }
    if !cfg.explicit.mode {
        cfg.mode = Mode::parse(&s.mode).unwrap_or(Mode::Plain);
    }
    if !cfg.explicit.hub_addr {
        cfg.hub_addr = s.hub_addr.clone();
    }
    if !cfg.explicit.web_addr {
        cfg.web_addr = s.web_addr.clone();
    }
    if !cfg.explicit.jwt_ttl {
        cfg.jwt_ttl = s.jwt_ttl;
    }
    if !cfg.explicit.require_jwt && s.jwt_enabled {
        cfg.require_jwt = true;
    }
    if !cfg.explicit.team && s.team.is_some() {
        cfg.team = Some(cfg.data_dir.join("team.json"));
    }
    if !cfg.explicit.nodes_doc {
        cfg.nodes_doc = s.nodes_doc.clone();
    }
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
    use std::io::IsTerminal;
    let mut cfg = parse_args();

    // --init-team：写出团队配置模板后退出（不启动服务）。
    if let Some(path) = cfg.init_team.clone() {
        match std::fs::write(&path, Team::template()) {
            Ok(_) => {
                println!("已写出工作团队配置模板: {}", path.display());
                eprintln!("[team] 请编辑该文件填写成员（name/role/note），然后以 `--team {}` 启动主节点。", path.display());
            }
            Err(e) => {
                eprintln!("[fatal] 写出团队模板 {} 失败: {e}", path.display());
                std::process::exit(1);
            }
        }
        return;
    }

    // 首次启动向导：--setup 强制；或首启（无 setup.json）且 stdin 为终端。
    // --issue-token / --no-setup / daemon 子进程不进入向导。
    let first_run = !cfg.data_dir.join("setup.json").exists();
    let want_wizard = cfg.issue_token.is_none()
        && !cfg.no_setup
        && !cfg.daemon_child
        && (cfg.setup || (first_run && std::io::stdin().is_terminal()));
    if want_wizard {
        std::fs::create_dir_all(&cfg.data_dir).expect("创建 data-dir 失败");
        let s = setup::run_wizard(&cfg.manager_name);
        if let Err(e) = s.save(&cfg.data_dir) {
            eprintln!("[fatal] {e}");
            std::process::exit(1);
        }
        apply_setup(&mut cfg, &s);
    } else if let Some(s) = setup::Setup::load(&cfg.data_dir) {
        eprintln!("[setup] 已加载首启配置 setup.json（deploy={} mode={}）", s.deploy, s.mode);
        apply_setup(&mut cfg, &s);
    }

    // 监听规则：禁止通配地址（0.0.0.0 / ::）+ 端口下限（≥ 10001）+ 部署模式规则。
    validate_bind("--hub-addr", &cfg.hub_addr);
    validate_bind("--web-addr", &cfg.web_addr);
    validate_deploy(&cfg);

    std::fs::create_dir_all(&cfg.data_dir).expect("创建 data-dir 失败");

    // 数据迁移检查（版本标记对比；当前版本间无结构变更）。
    update::check_migration(&cfg.data_dir);

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

    // --issue-token：签发令牌后打印并退出（不启动服务）；并入 tokens.json 供节点文档引用。
    if let Some(name) = cfg.issue_token.clone() {
        let a = auth.as_ref().expect("issue-token 需要 JWT 校验器");
        let token = a.issue(&name);
        println!("{token}");
        let mut m = HashMap::new();
        m.insert(name.clone(), token);
        nodesdoc::save_tokens(&cfg.data_dir, &m);
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

    // WS 消息枢纽（端口占用自动 +1）；先绑定，便于把真实地址写入团队提示词。
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

    // 工作团队：加载定义（成员名唯一已在解析时校验；不得与管理者名冲突）。
    let team_def = match &cfg.team {
        Some(path) => match Team::load(path) {
            Ok(t) => {
                if let Some(m) = t.members.iter().find(|m| m.name == cfg.manager_name) {
                    eprintln!("[fatal] 团队成员 {} 与主节点管理者名冲突（节点名称不可重复）。", m.name);
                    std::process::exit(1);
                }
                Some(t)
            }
            Err(e) => {
                eprintln!("[fatal] {e}");
                std::process::exit(1);
            }
        },
        None => None,
    };

    // 团队令牌：每次启动为全体会员重新签发（保证有效期充足），落盘 tokens.json（0600）。
    let mut tokens: HashMap<String, String> = HashMap::new();
    if jwt {
        if let (Some(t), Some(a)) = (&team_def, &auth) {
            for m in &t.members {
                tokens.insert(m.name.clone(), a.issue(&m.name));
            }
            nodesdoc::save_tokens(&cfg.data_dir, &tokens);
        }
    }

    let jctx = join_ctx(&cfg, mode, &hub_addr, jwt);
    let team_view = team_def.as_ref().map(|t| Arc::new(t.view_with_tokens(&jctx, &tokens)));

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
        team_view.clone(),
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

    // 节点接入文档：生成 + 动态更新（节点上下线/令牌/Tunnel 变化）。
    let scheme = if mode == Mode::Tls { "wss" } else { "ws" };
    let web_scheme = if mode == Mode::Tls { "https" } else { "http" };
    let doc_path = nodesdoc::resolve_doc_path(cfg.nodes_doc.as_deref());
    nodesdoc::ensure_gitignore();
    let doc_ctx = nodesdoc::DocCtx {
        doc_path: doc_path.clone(),
        manager_name: cfg.manager_name.clone(),
        deploy: cfg.deploy.as_str().to_string(),
        mode: mode.as_str().to_string(),
        server: format!("{scheme}://{hub_addr}"),
        web: format!("{web_scheme}://{web_addr}"),
        data_dir: cfg.data_dir.display().to_string(),
        team: team_def.clone(),
        jwt,
        tunnel_url: cfg.tunnel_url.clone(),
    };
    nodesdoc::spawn_updater(state.clone(), doc_ctx);

    // 每周更新检查（后台阻塞任务，不拖慢启动）。
    {
        let state = state.clone();
        let data_dir = cfg.data_dir.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(v) = update::check_weekly(&data_dir) {
                if v["update_available"].as_bool().unwrap_or(false) {
                    eprintln!(
                        "[update] 发现新版本 {}（当前 {}）：请运行 scripts/starfruit-update.sh 完成更新（含数据迁移检查）。仓库: {}",
                        v["latest_version"].as_str().unwrap_or("?"),
                        update::CURRENT_VERSION,
                        update::REPO_URL
                    );
                }
                state.emit("update", v.clone());
                let state2 = state.clone();
                tokio::spawn(async move {
                    *state2.update.lock().await = Some(v);
                });
            }
        });
    }

    // 明文监听非本机地址时给出提示：公网务必用 Cloudflare Tunnel + tls + jwt。
    if mode == Mode::Plain && !is_loopback_host(host_of(&hub_addr)) {
        eprintln!(
            "[warn] 当前以明文(plain)监听非本机地址 {}：仅建议可信局域网使用；\
公网请改用 Cloudflare Tunnel 暴露并配合 --mode tls + JWT 令牌。",
            hub_addr
        );
    }

    // 启动成功说明（会话提示）：地址端口 / 团队成员与令牌 / 节点文档路径。
    let tunnel = cfg
        .tunnel_url
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(paths::read_tunnel_url);
    print_banner(&cfg, &hub_addr, &web_addr, jwt, tunnel.as_deref(), &doc_path, &tokens);

    // 工作团队：已配置则打印提示词（含真实令牌）并落盘；未配置则提示用户可选配置。
    match (&team_def, &team_view) {
        (Some(t), Some(view)) => {
            team::print_prompts(t, &view.prompts);
            let path = team::prompts_path(&cfg.data_dir);
            match std::fs::write(&path, &view.markdown) {
                Ok(_) => {
                    eprintln!(
                        "[team] 提示词已落盘 {}（面板查看: {web_scheme}://{web_addr}/team）",
                        path.display()
                    );
                }
                Err(e) => eprintln!("[team] 写入 {} 失败: {e}", path.display()),
            }
        }
        _ => team::print_reminder(),
    }

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