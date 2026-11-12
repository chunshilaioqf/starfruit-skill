//! ws-hub：本机轻量 WS 消息枢纽。
//!
//! 供 ZCode（总管）与 Trae / zcode-1（执行者）在本机直连互发 JSON 消息。
//! - 仅绑定 127.0.0.1，无鉴权（用户明确要求）
//! - 星型拓扑：所有客户端连到本进程，按 `to` 字段路由或广播
//! - v1.1：离线消息队列——对端不在线时按目标名暂存（每对端上限
//!   [`BACKLOG_CAP`] 条，超限丢弃最旧并回执 `warn`），对端注册成功后自动
//!   投递并逐条向原发送方回执 `queued_delivered`；发送暂存即回执 `queued`
//!   （区别于即时 `sent`）；注册回执带 `backlog` 字段
//! - 既有回执（registered/sent/no_such_peer/broadcast_ok/bad_msg）保持兼容
//! - 协议文档见项目根 `ws_protocol.md`

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// 在线注册表：名字 -> 该连接的写通道。
type Peers = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Message>>>>;

/// 离线消息队列（按目标名共享，跨重连保留）。
type SharedBacklog = Arc<Mutex<Backlog>>;

const DEFAULT_ADDR: &str = "127.0.0.1:9501";

/// 每个对端的离线暂存上限；超限丢弃最旧并回执 `warn`。
pub const BACKLOG_CAP: usize = 200;

fn main() {
    let addr = std::env::var("WS_HUB_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("构建 tokio runtime 失败")
        .block_on(run(addr));
}

async fn run(addr: String) {
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("ws-hub 无法绑定 {addr}: {e}"));
    eprintln!("[ws-hub] listening on ws://{addr}");
    let peers: Peers = Arc::new(Mutex::new(HashMap::new()));
    let backlog: SharedBacklog = Arc::new(Mutex::new(Backlog::new()));
    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                let peers = peers.clone();
                let backlog = backlog.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve(stream, peers, backlog).await {
                        eprintln!("[ws-hub] {peer_addr} 连接结束: {e}");
                    }
                });
            }
            Err(e) => eprintln!("[ws-hub] accept 失败: {e}"),
        }
    }
}

/// 系统回执消息 JSON。
fn sys_json(code: &str, detail: &str) -> String {
    serde_json::json!({ "system": true, "code": code, "detail": detail }).to_string()
}

/// 注册回执 JSON（v1.1：附 `backlog` = 该名字的离线暂存条数）。
fn registered_json(name: &str, backlog: usize) -> String {
    serde_json::json!({
        "system": true,
        "code": "registered",
        "detail": name,
        "backlog": backlog,
    })
    .to_string()
}

/// 暂存的一条离线消息（原文 + 原发送方，投递后用于回执 `queued_delivered`）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stored {
    from: String,
    text: String,
}

/// 离线消息队列：每目标名一个 FIFO（[`BACKLOG_CAP`] 上限，超限淘汰最旧）。
///
/// 与在线注册表解耦：队列按**名字**归属，跨断线/重连保留；注册成功时由
/// 连接处理路径 [`Backlog::drain`] 取走全部待投递消息。
#[derive(Debug, Default)]
struct Backlog {
    queues: HashMap<String, VecDeque<Stored>>,
}

impl Backlog {
    fn new() -> Self {
        Self::default()
    }

    /// 某目标当前的暂存条数（backlog 计数，注册回执用）。
    fn backlog_of(&self, name: &str) -> usize {
        self.queues.get(name).map_or(0, VecDeque::len)
    }

    /// 入队一条离线消息；队列已满时淘汰最旧一条并返回之（供回执 `warn`）。
    fn push(&mut self, to: &str, from: &str, text: String) -> Option<Stored> {
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
    fn drain(&mut self, name: &str) -> Vec<Stored> {
        self.queues
            .remove(name)
            .map(|q| q.into_iter().collect())
            .unwrap_or_default()
    }
}

async fn serve(
    stream: TcpStream,
    peers: Peers,
    backlog: SharedBacklog,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws: WebSocketStream<TcpStream> = tokio_tungstenite::accept_async(stream).await?;
    eprintln!("[ws-hub] 握手完成");
    let (mut sink, mut incoming) = ws.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();

    // 写任务：把路由到本连接的消息顺序写出。
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut my_name: Option<String> = None;

    // 注意：读取错误必须在这里就地 break（不能用 `?` 提前返回），
    // 否则会跳过函数末尾的注册表清理，留下僵尸条目。
    while let Some(frame) = incoming.next().await {
        match frame {
            Err(_) => break,
            Ok(Message::Close(_)) => break,
            Ok(Message::Text(text)) => {
                eprintln!("[ws-hub] 收到文本帧: {:.120}", text);
                let v: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = out_tx.send(Message::text(sys_json(
                            "bad_msg",
                            &format!("JSON 解析失败: {e}"),
                        )));
                        continue;
                    }
                };

                // 注册帧：{"action":"register","from":"<名字>"}
                if v.get("action").and_then(|a| a.as_str()) == Some("register") {
                    let name = v
                        .get("from")
                        .and_then(|f| f.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if name.is_empty() {
                        let _ =
                            out_tx.send(Message::text(sys_json("bad_msg", "register 缺少 from")));
                        continue;
                    }
                    let mut map = peers.lock().await;
                    // 同名重连：顶掉旧连接（向旧连接发 Close），本连接接管名字。
                    if let Some(old) = map.insert(name.clone(), out_tx.clone()) {
                        if !old.same_channel(&out_tx) {
                            let _ = old.send(Message::Close(None));
                            eprintln!("[ws-hub] {name} 重连，已顶掉旧连接");
                        }
                    }
                    my_name = Some(name.clone());
                    let online = map.len();

                    // v1.1：注册成功即自动投递该名字的离线积压（回执带 backlog 数）。
                    let (pending, waited) = {
                        let mut b = backlog.lock().await;
                        let n = b.backlog_of(&name);
                        (b.drain(&name), n)
                    };
                    let _ = out_tx.send(Message::text(registered_json(&name, waited)));
                    eprintln!("[ws-hub] 注册: {name}（在线 {online}，backlog {waited}）");

                    if !pending.is_empty() {
                        let total = pending.len();
                        for (i, msg) in pending.iter().enumerate() {
                            let _ = out_tx.send(Message::text(msg.text.clone()));
                            // 逐条向原发送方回执 queued_delivered（发送方不在线则跳过）
                            if let Some(sender) = map.get(&msg.from) {
                                let _ = sender.send(Message::text(sys_json(
                                    "queued_delivered",
                                    &format!("to={name} seq={}/{}", i + 1, total),
                                )));
                            }
                        }
                        eprintln!("[ws-hub] {name} 上线，投递离线积压 {total} 条");
                    }
                    drop(map);
                    continue;
                }

                // 转发帧：{"from":"...","to":"<名字>|all",...其余字段原样透传}
                let to = v
                    .get("to")
                    .and_then(|t| t.as_str())
                    .unwrap_or("all")
                    .to_string();
                let from = v
                    .get("from")
                    .and_then(|f| f.as_str())
                    .unwrap_or("(unknown)")
                    .to_string();

                let map = peers.lock().await;
                if to == "all" {
                    let targets = map
                        .iter()
                        .filter(|(name, _)| Some(name.as_str()) != my_name.as_deref())
                        .count();
                    for (name, sender) in map.iter() {
                        if Some(name.as_str()) != my_name.as_deref() {
                            let _ = sender.send(Message::text(text.clone()));
                        }
                    }
                    drop(map);
                    let _ = out_tx.send(Message::text(sys_json(
                        "broadcast_ok",
                        &format!("from={from} targets={targets}"),
                    )));
                    eprintln!("[ws-hub] 广播 from={from} -> {targets} 个在线");
                } else if Some(to.as_str()) == my_name.as_deref() {
                    drop(map);
                    let _ = out_tx.send(Message::text(sys_json("bad_msg", "不能发给自己")));
                } else if let Some(sender) = map.get(&to) {
                    let _ = sender.send(Message::text(text.clone()));
                    drop(map);
                    let _ = out_tx.send(Message::text(sys_json("sent", &format!("to={to}"))));
                    eprintln!("[ws-hub] {from} -> {to}");
                } else {
                    // v1.1：对端不在线 → 暂存（上限淘汰最旧并回执 warn），回执 queued。
                    drop(map);
                    let evicted = {
                        let mut b = backlog.lock().await;
                        b.push(&to, &from, text.clone())
                    };
                    if let Some(old) = evicted {
                        let _ = out_tx.send(Message::text(sys_json(
                            "warn",
                            &format!(
                                "to={to} backlog 已满({BACKLOG_CAP})，丢弃最旧: {:.40}",
                                old.text
                            ),
                        )));
                        eprintln!("[ws-hub] {to} 暂存已满，淘汰最旧一条");
                    }
                    let waited = {
                        let b = backlog.lock().await;
                        b.backlog_of(&to)
                    };
                    let _ = out_tx.send(Message::text(sys_json(
                        "queued",
                        &format!("to={to} 对端不在线，已暂存（当前 {waited}/{BACKLOG_CAP}）"),
                    )));
                    eprintln!("[ws-hub] {from} -> {to} 暂存（backlog {waited}）");
                }
            }
            // Ping 由 tungstenite 读路径自动回 Pong；二进制帧本协议不使用。
            _ => {}
        }
    }

    // 清理：仅当注册表里仍是自己这条连接时才移除（防止误删顶替者的注册）。
    if let Some(name) = &my_name {
        let mut map = peers.lock().await;
        if map
            .get(name)
            .map(|s| s.same_channel(&out_tx))
            .unwrap_or(false)
        {
            map.remove(name);
            let online = map.len();
            drop(map);
            eprintln!("[ws-hub] 离线: {name}（在线 {online}）");
        }
    }
    drop(out_tx);
    let _ = writer.await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(from: &str, text: &str) -> Stored {
        Stored {
            from: from.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn 离线暂存_入队并保序() {
        let mut b = Backlog::new();
        assert_eq!(b.backlog_of("trae"), 0);
        assert!(b.push("trae", "zcode", "任务一".into()).is_none());
        assert!(b.push("trae", "zcode", "任务二".into()).is_none());
        assert_eq!(b.backlog_of("trae"), 2, "两条均暂存");

        let drained = b.drain("trae");
        assert_eq!(
            drained,
            vec![stored("zcode", "任务一"), stored("zcode", "任务二")],
            "FIFO 顺序，且保留原发送方"
        );
        // 其他目标的队列不受影响
        assert_eq!(b.backlog_of("trae"), 0, "drain 后清空");
        assert!(b.push("trae", "zcode", "任务三".into()).is_none());
        assert_eq!(b.drain("zcode-1"), Vec::new(), "未暂存目标 drain 为空");
    }

    #[test]
    fn 上线投递_drain_取走全部并清空_backlog_归零() {
        let mut b = Backlog::new();
        for i in 0..5 {
            b.push("trae", "zcode", format!("m{i}"));
        }
        assert_eq!(b.backlog_of("trae"), 5);
        let pending = b.drain("trae");
        assert_eq!(pending.len(), 5);
        assert_eq!(pending[0].text, "m0");
        assert_eq!(pending[4].text, "m4");
        assert_eq!(b.backlog_of("trae"), 0, "投递后 backlog 计数归零");
        assert!(b.drain("trae").is_empty(), "重复 drain 无残留");
    }

    #[test]
    fn 上限淘汰_第201条挤掉最旧并返回被淘汰者() {
        let mut b = Backlog::new();
        for i in 0..BACKLOG_CAP {
            assert!(b.push("trae", "zcode", format!("m{i}")).is_none());
        }
        assert_eq!(b.backlog_of("trae"), BACKLOG_CAP);
        let evicted = b.push("trae", "zcode", "最新一条".into());
        assert_eq!(
            evicted,
            Some(stored("zcode", "m0")),
            "超限淘汰最旧（第 1 条）并返回给回执 warn 用"
        );
        assert_eq!(b.backlog_of("trae"), BACKLOG_CAP, "淘汰后仍保持上限");
        let first = b.drain("trae");
        assert_eq!(first[0].text, "m1", "被淘汰的 m0 不再出现");
        assert_eq!(first[BACKLOG_CAP - 1].text, "最新一条");
    }

    #[test]
    fn backlog计数_各目标独立且注册回执带数() {
        let mut b = Backlog::new();
        b.push("trae", "zcode", "a".into());
        b.push("trae", "zcode", "b".into());
        b.push("zcode-1", "zcode", "c".into());
        assert_eq!(b.backlog_of("trae"), 2);
        assert_eq!(b.backlog_of("zcode-1"), 1);
        assert_eq!(b.backlog_of("nobody"), 0);

        // 注册回执 JSON：既有消费方读 code/detail 不受影响，新增 backlog 字段
        let j: serde_json::Value = serde_json::from_str(&registered_json("trae", 2)).unwrap();
        assert_eq!(j["system"], serde_json::Value::Bool(true));
        assert_eq!(j["code"], "registered");
        assert_eq!(j["detail"], "trae");
        assert_eq!(j["backlog"], 2);
        // backlog 为 0 时字段仍在（消费方无需判缺）
        let j0: serde_json::Value = serde_json::from_str(&registered_json("trae", 0)).unwrap();
        assert_eq!(j0["backlog"], 0);
    }
}
