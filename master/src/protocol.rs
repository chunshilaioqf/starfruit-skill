//! starfruit 协议公共定义：运行模式、消息结构、嗅探判定。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;

/// 运行模式：非加密（ws）/ 加密（wss）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Plain,
    Tls,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "plain" | "ws" => Some(Mode::Plain),
            "tls" | "wss" => Some(Mode::Tls),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Plain => "plain",
            Mode::Tls => "tls",
        }
    }
}

/// 部署模式：决定监听地址范围与安全要求（tls / jwt 是否必选）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deploy {
    /// 本机模式：只监听 127.0.0.1；tls 与 jwt 均为可选项。
    Local,
    /// 局域网模式：监听本机局域网 IP；tls 与 jwt 均为可选项。
    Lan,
    /// 公网模式：必须指定具体 IP（禁止 0.0.0.0）；tls 与 jwt 均为必选项。
    Public,
}

impl Deploy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "local" | "本机" => Some(Deploy::Local),
            "lan" | "局域网" => Some(Deploy::Lan),
            "public" | "公网" => Some(Deploy::Public),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Deploy::Local => "local",
            Deploy::Lan => "lan",
            Deploy::Public => "public",
        }
    }

    /// 中文描述（启动说明用）。
    pub fn desc(&self) -> &'static str {
        match self {
            Deploy::Local => "本机模式（只监听 127.0.0.1；tls / jwt 可选）",
            Deploy::Lan => "局域网模式（监听本机局域网 IP；tls / jwt 可选）",
            Deploy::Public => "公网模式（须指定具体 IP；tls / jwt 必选）",
        }
    }
}

/// 首字节嗅探判定：连接模式与服务端不符时的处置。
#[derive(Debug, PartialEq, Eq)]
pub enum Sniff {
    /// 首字节与服务端模式一致，继续正常握手。
    Proceed,
    /// 模式不匹配：向对方回明文错误行后断开。
    Reject(&'static str),
}

/// 传输层嗅探：TLS 流量首字节固定是 0x16（TLS Handshake record）。
///
/// - `plain` 服务端收到 `0x16` → 对方在用 TLS 连明文口；
/// - `tls` 服务端收到其它首字节 → 对方在用明文连加密口（明文错误行对方可直接读取）。
pub fn sniff(first_byte: u8, server_mode: Mode) -> Sniff {
    match (server_mode, first_byte) {
        (Mode::Plain, 0x16) => Sniff::Reject(
            "STARFRUIT_ERR mode_mismatch: 服务端为非加密模式(ws://)，客户端却使用 TLS。请以 --mode plain 启动节点后重连。",
        ),
        (Mode::Tls, b) if b != 0x16 => Sniff::Reject(
            "STARFRUIT_ERR mode_mismatch: 服务端为加密模式(wss://)，客户端却使用明文。请以 --mode tls 启动节点后重连。",
        ),
        _ => Sniff::Proceed,
    }
}

/// 在线连接注册表：名字 -> 该连接的写通道（名称全网唯一：同名在线注册被拒绝 name_conflict）。
pub type Peers = Arc<Mutex<std::collections::HashMap<String, mpsc::UnboundedSender<Message>>>>;

/// 离线消息队列（按目标名共享，跨重连保留；v1.1 语义）。
pub type SharedBacklog = Arc<Mutex<Backlog>>;

/// 每个对端的离线暂存上限；超限丢弃最旧并回执 `warn`。
pub const BACKLOG_CAP: usize = 200;

/// 暂存的一条离线消息（原文 + 原发送方，投递后用于回执 `queued_delivered`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub from: String,
    pub text: String,
}

/// 离线消息队列：每目标名一个 FIFO（[`BACKLOG_CAP`] 上限，超限淘汰最旧）。
#[derive(Debug, Default)]
pub struct Backlog {
    queues: std::collections::HashMap<String, std::collections::VecDeque<Stored>>,
}

impl Backlog {
    pub fn new() -> Self {
        Self::default()
    }

    /// 某目标当前的暂存条数（backlog 计数，注册回执用）。
    pub fn backlog_of(&self, name: &str) -> usize {
        self.queues.get(name).map_or(0, std::collections::VecDeque::len)
    }

    /// 入队一条离线消息；队列已满时淘汰最旧一条并返回之（供回执 `warn`）。
    pub fn push(&mut self, to: &str, from: &str, text: String) -> Option<Stored> {
        let q = self.queues.entry(to.to_string()).or_default();
        let mut evicted = None;
        if q.len() >= BACKLOG_CAP {
            evicted = q.pop_front();
        }
        q.push_back(Stored {
            from: from.to_string(),
            text,
        });
        evicted
    }

    /// 取走该目标的全部待投递消息（FIFO 顺序），队列清空。
    pub fn drain(&mut self, name: &str) -> Vec<Stored> {
        self.queues
            .remove(name)
            .map(|q| q.into_iter().collect())
            .unwrap_or_default()
    }
}

/// 节点元信息（Web 面板展示 + 统计）。
#[derive(Debug, Clone)]
pub struct NodeMeta {
    pub role: String,
    pub mode: String,
    pub online: bool,
    pub connected_at_ms: u64,
    pub msgs_out: u64,
    pub msgs_in: u64,
    /// 主节点内置的管理者端点（不占用独立连接）。
    pub internal: bool,
}

impl NodeMeta {
    pub fn new(role: &str, mode: &str, internal: bool) -> Self {
        Self {
            role: role.to_string(),
            mode: mode.to_string(),
            online: true,
            connected_at_ms: now_ms(),
            msgs_out: 0,
            msgs_in: 0,
            internal,
        }
    }
}

/// 消息历史：环形缓冲 + 可选 JSONL 落盘。
#[derive(Debug)]
pub struct History {
    pub seq: u64,
    pub cap: usize,
    pub records: std::collections::VecDeque<Value>,
    pub file: Option<std::fs::File>,
}

impl History {
    /// 追加一条记录，返回其 `seq`；超出容量淘汰最旧。
    pub fn push(&mut self, mut rec: Value) -> u64 {
        self.seq += 1;
        rec["seq"] = Value::from(self.seq);
        if let Some(f) = self.file.as_mut() {
            use std::io::Write;
            let _ = writeln!(f, "{rec}");
        }
        self.records.push_back(rec);
        while self.records.len() > self.cap {
            self.records.pop_front();
        }
        self.seq
    }
}

/// 共享运行状态：连接表 / 离线队列 / 节点注册表 / 历史 / 事件总线。
pub struct State {
    pub peers: Peers,
    pub backlog: SharedBacklog,
    pub registry: Mutex<std::collections::HashMap<String, NodeMeta>>,
    pub history: Mutex<History>,
    pub events: broadcast::Sender<Value>,
    pub started_at_ms: u64,
    pub msg_total: AtomicU64,
    /// 每周更新检查结果（update.rs 写入，面板 /api/stats 展示）。
    pub update: Mutex<Option<Value>>,
}

impl State {
    pub fn new(history_cap: usize, history_file: Option<std::fs::File>) -> Arc<Self> {
        let (events, _) = broadcast::channel(1024);
        Arc::new(Self {
            peers: Arc::new(Mutex::new(std::collections::HashMap::new())),
            backlog: Arc::new(Mutex::new(Backlog::new())),
            registry: Mutex::new(std::collections::HashMap::new()),
            history: Mutex::new(History {
                seq: 0,
                cap: history_cap,
                records: std::collections::VecDeque::new(),
                file: history_file,
            }),
            events,
            started_at_ms: now_ms(),
            msg_total: AtomicU64::new(0),
            update: Mutex::new(None),
        })
    }

    /// 记录一条消息并广播面板事件（SSE）。
    pub async fn record(&self, rec: Value) -> u64 {
        self.msg_total.fetch_add(1, Ordering::Relaxed);
        let seq = self.history.lock().await.push(rec.clone());
        let _ = self.events.send(serde_json::json!({ "ev": "message", "data": rec }));
        seq
    }

    /// 面板事件（节点上线/下线等）。
    pub fn emit(&self, ev: &str, data: Value) {
        let _ = self.events.send(serde_json::json!({ "ev": ev, "data": data }));
    }
}

/// 当前墙钟毫秒（epoch）。
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 系统回执消息 JSON（v1.1 兼容）。
pub fn sys_json(code: &str, detail: &str) -> String {
    serde_json::json!({ "system": true, "code": code, "detail": detail }).to_string()
}

/// 注册回执 JSON（v2：附 backlog / mode / ver）。
pub fn registered_json(name: &str, backlog: usize, mode: &str, ver: u64) -> String {
    serde_json::json!({
        "system": true,
        "code": "registered",
        "detail": name,
        "backlog": backlog,
        "mode": mode,
        "ver": ver,
    })
    .to_string()
}
