//! hub 核心：连接处理、注册、消息路由（v1.1 语义 + v2 双模式/历史/事件）。

use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::protocol::{
    now_ms, registered_json, sniff, sys_json, Mode, Sniff, State, BACKLOG_CAP,
};

/// 路由结果回执（返回给发送方的 `system` 帧 JSON）。
/// 路由逻辑与连接层解耦：内置管理者端点与 WS 连接共用。
/// `v` 为发送方原始帧；转发时 `from/to/type/text` 以服务端判定为准，其余字段原样透传。
pub async fn route(state: &Arc<State>, from: &str, v: &Value) -> String {
    let to = v
        .get("to")
        .and_then(|t| t.as_str())
        .unwrap_or("all")
        .to_string();
    let kind = v
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("chat")
        .to_string();
    let ts = v.get("ts").and_then(|t| t.as_str()).unwrap_or("").to_string();
    let text = v
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    // 转发载荷：接收方拿到的是完整 JSON 帧（from/to/type/text + 其余透传字段）。
    let mut frame = v.clone();
    frame["from"] = Value::from(from);
    frame["to"] = Value::from(to.as_str());
    frame["type"] = Value::from(kind.as_str());
    frame["text"] = Value::from(text.as_str());
    let payload = frame.to_string();

    // 记录（面板/历史）：无论投递结果如何，路由行为都留痕。
    let status = if to == "all" { "broadcast" } else { "sent" };
    state
        .record(serde_json::json!({
            "from": from, "to": to, "type": kind, "text": text,
            "ts": ts, "ts_ms": now_ms(), "status": status,
        }))
        .await;

    // 发送方计数。
    {
        let mut reg = state.registry.lock().await;
        if let Some(m) = reg.get_mut(from) {
            m.msgs_out += 1;
        }
    }

    if to == "all" {
        let map = state.peers.lock().await;
        let targets = map
            .iter()
            .filter(|(name, _)| name.as_str() != from)
            .count();
        for (name, sender) in map.iter() {
            if name.as_str() != from {
                let _ = sender.send(Message::text(payload.clone()));
                let mut reg = state.registry.lock().await;
                if let Some(m) = reg.get_mut(name.as_str()) {
                    m.msgs_in += 1;
                }
            }
        }
        drop(map);
        sys_json("broadcast_ok", &format!("from={from} targets={targets}"))
    } else if to == from {
        sys_json("bad_msg", "不能发给自己")
    } else {
        // 已注册名字（含内置管理者）才允许暂存；否则 no_such_peer。
        let known = {
            let reg = state.registry.lock().await;
            reg.contains_key(&to)
        };
        let map = state.peers.lock().await;
        if let Some(sender) = map.get(&to) {
            let _ = sender.send(Message::text(payload.clone()));
            drop(map);
            let mut reg = state.registry.lock().await;
            if let Some(m) = reg.get_mut(&to) {
                m.msgs_in += 1;
            }
            sys_json("sent", &format!("to={to}"))
        } else if !known {
            drop(map);
            let online: Vec<String> = {
                let reg = state.registry.lock().await;
                reg.iter()
                    .filter(|(_, m)| m.online)
                    .map(|(n, _)| n.clone())
                    .collect()
            };
            sys_json(
                "no_such_peer",
                &format!("to={to} 未注册；当前在线: {}", online.join(",")),
            )
        } else {
            // v1.1：对端已注册但不在线 → 暂存（上限淘汰最旧并回执 warn），回执 queued。
            drop(map);
            let evicted = {
                let mut b = state.backlog.lock().await;
                b.push(&to, from, payload)
            };
            let mut warn_note = String::new();
            if let Some(old) = evicted {
                warn_note = format!(
                    "；backlog 已满({BACKLOG_CAP})，丢弃最旧: {:.40}",
                    old.text
                );
            }
            let waited = {
                let b = state.backlog.lock().await;
                b.backlog_of(&to)
            };
            sys_json(
                "queued",
                &format!("to={to} 对端不在线，已暂存（当前 {waited}/{BACKLOG_CAP}）{warn_note}"),
            )
        }
    }
}

/// 单个连接的完整生命周期：嗅探 → 可选 TLS → WS 握手 → 注册/路由 → 清理。
pub async fn serve(
    raw: TcpStream,
    state: Arc<State>,
    mode: Mode,
    tls: Option<tokio_rustls::TlsAcceptor>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // ① 传输层嗅探：加密方式不对 → 明文报错后断开（对方可读时必读到）。
    let mut head = [0u8; 1];
    let n = raw.peek(&mut head).await?;
    if n == 0 {
        return Ok(());
    }
    if let Sniff::Reject(msg) = sniff(head[0], mode) {
        let mut raw = raw;
        let _ = raw.write_all(msg.as_bytes()).await;
        let _ = raw.write_all(b"\r\n").await;
        let _ = raw.flush().await;
        let _ = raw.shutdown().await;
        eprintln!("[hub] 模式不匹配，已向对方报错并断开");
        return Ok(());
    }

    // ② 按模式完成传输层握手。
    enum Stream {
        Plain(Box<WebSocketStream<TcpStream>>),
        Tls(Box<WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>>),
    }
    let ws = match (mode, tls) {
        (Mode::Tls, Some(acc)) => {
            let tls_stream = acc.accept(raw).await?;
            Stream::Tls(Box::new(tokio_tungstenite::accept_async(tls_stream).await?))
        }
        _ => Stream::Plain(Box::new(tokio_tungstenite::accept_async(raw).await?)),
    };

    // ③ WS 帧循环（v1.1 语义 + v2 注册校验）。
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Message>();
    let mut my_name: Option<String> = None;

    macro_rules! split_loop {
        ($sink:expr, $incoming:expr) => {{
            let writer = tokio::spawn(async move {
                let mut sink = $sink;
                while let Some(msg) = out_rx.recv().await {
                    if sink.send(msg).await.is_err() {
                        break;
                    }
                }
            });
            let mut incoming = $incoming;
            while let Some(frame) = incoming.next().await {
                match frame {
                    Err(_) => break,
                    Ok(Message::Close(_)) => break,
                    Ok(Message::Text(text)) => {
                        if let Some(done) = handle_text(
                            &state, &out_tx, &mut my_name, &text, mode,
                        )
                        .await
                        {
                            if done {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
            writer.abort();
        }};
    }

    match ws {
        Stream::Plain(w) => {
            let (sink, incoming) = (*w).split();
            split_loop!(sink, incoming);
        }
        Stream::Tls(w) => {
            let (sink, incoming) = (*w).split();
            split_loop!(sink, incoming);
        }
    }

    // ④ 清理：仅当注册表里仍是自己这条连接时才移除（防止误删顶替者的注册）。
    if let Some(name) = &my_name {
        let mut map = state.peers.lock().await;
        if map
            .get(name)
            .map(|s| s.same_channel(&out_tx))
            .unwrap_or(false)
        {
            map.remove(name);
            drop(map);
            {
                let mut reg = state.registry.lock().await;
                if let Some(m) = reg.get_mut(name.as_str()) {
                    m.online = false;
                }
            }
            let online = state.peers.lock().await.len();
            eprintln!("[hub] 离线: {name}（在线 {online}）");
            state.emit(
                "node_leave",
                serde_json::json!({ "name": name, "ts_ms": now_ms() }),
            );
        }
    }
    Ok(())
}

/// 处理一条文本帧；返回 `Some(true)` 表示应关闭连接。
async fn handle_text(
    state: &Arc<State>,
    out_tx: &mpsc::UnboundedSender<Message>,
    my_name: &mut Option<String>,
    text: &str,
    mode: Mode,
) -> Option<bool> {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            let _ = out_tx.send(Message::text(sys_json(
                "bad_msg",
                &format!("JSON 解析失败: {e}"),
            )));
            return None;
        }
    };

    // 注册帧：{"action":"register","from":"<名字>","role":?,"mode":?,"ver":?}
    if v.get("action").and_then(|a| a.as_str()) == Some("register") {
        let name = v
            .get("from")
            .and_then(|f| f.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() {
            let _ = out_tx.send(Message::text(sys_json("bad_msg", "register 缺少 from")));
            return None;
        }
        // v2：注册帧声明的加密模式必须与服务端一致，否则向对方报错并断开。
        if let Some(declared) = v.get("mode").and_then(|m| m.as_str()) {
            match Mode::parse(declared) {
                None => {
                    let _ = out_tx.send(Message::text(sys_json(
                        "bad_msg",
                        "mode 只能是 plain|tls",
                    )));
                    return None;
                }
                Some(m) if m != mode => {
                    let _ = out_tx.send(Message::text(sys_json(
                        "mode_mismatch",
                        &format!(
                            "server={} client={declared}，加密方式不一致，请以 --mode {} 启动节点后重连",
                            mode.as_str(),
                            mode.as_str()
                        ),
                    )));
                    // 报错帧必须先落到对端：连同 Close 一起入队，由写任务按序冲刷后再断开。
                    let _ = out_tx.send(Message::Close(None));
                    eprintln!("[hub] {name} 加密模式不匹配（server={} client={declared}），已报错断开", mode.as_str());
                    return None;
                }
                _ => {}
            }
        }
        let role = v
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("worker")
            .to_string();
        let ver = v.get("ver").and_then(|r| r.as_u64()).unwrap_or(1);

        let mut map = state.peers.lock().await;
        // 同名重连：顶掉旧连接（向旧连接发 Close），本连接接管名字。
        if let Some(old) = map.insert(name.clone(), out_tx.clone()) {
            if !old.same_channel(out_tx) {
                let _ = old.send(Message::Close(None));
                eprintln!("[hub] {name} 重连，已顶掉旧连接");
            }
        }
        *my_name = Some(name.clone());
        let online = map.len();

        // 注册成功即自动投递该名字的离线积压（v1.1）。
        let (pending, waited) = {
            let mut b = state.backlog.lock().await;
            let n = b.backlog_of(&name);
            (b.drain(&name), n)
        };
        {
            let mut reg = state.registry.lock().await;
            let meta = reg
                .entry(name.clone())
                .or_insert_with(|| crate::protocol::NodeMeta::new(&role, mode.as_str(), false));
            meta.role = role.clone();
            meta.online = true;
            meta.connected_at_ms = now_ms();
        }
        let _ = out_tx.send(Message::text(registered_json(
            &name,
            waited,
            mode.as_str(),
            ver,
        )));
        eprintln!("[hub] 注册: {name}（role={role}，在线 {online}，backlog {waited}）");
        state.emit(
            "node_join",
            serde_json::json!({ "name": name, "role": role, "ts_ms": now_ms() }),
        );

        if !pending.is_empty() {
            let total = pending.len();
            for (i, msg) in pending.iter().enumerate() {
                let _ = out_tx.send(Message::text(msg.text.clone()));
                state
                    .record(serde_json::json!({
                        "from": msg.from, "to": name, "type": "chat",
                        "text": msg.text, "ts_ms": now_ms(),
                        "status": "queued_delivered",
                    }))
                    .await;
                if let Some(sender) = map.get(&msg.from) {
                    let _ = sender.send(Message::text(sys_json(
                        "queued_delivered",
                        &format!("to={name} seq={}/{}", i + 1, total),
                    )));
                }
            }
            eprintln!("[hub] {name} 上线，投递离线积压 {total} 条");
        }
        return None;
    }

    // 转发帧：{"from":"...","to":"<名字>|all",...其余字段原样透传（v1.1 兼容）
    let from = my_name
        .clone()
        .or_else(|| v.get("from").and_then(|f| f.as_str()).map(str::to_string))
        .unwrap_or_else(|| "(unknown)".to_string());
    let receipt = route(state, &from, &v).await;
    let _ = out_tx.send(Message::text(receipt));
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Backlog;

    #[test]
    fn 嗅探_明文服务端收到_tls_首字节应拒绝() {
        assert_eq!(
            sniff(0x16, Mode::Plain),
            Sniff::Reject(
                "STARFRUIT_ERR mode_mismatch: 服务端为非加密模式(ws://)，客户端却使用 TLS。请以 --mode plain 启动节点后重连。"
            )
        );
    }

    #[test]
    fn 嗅探_tls_服务端收到明文首字节应拒绝() {
        assert!(matches!(sniff(b'G', Mode::Tls), Sniff::Reject(_)));
        assert!(matches!(sniff(b'P', Mode::Tls), Sniff::Reject(_)));
    }

    #[test]
    fn 嗅探_模式一致应放行() {
        assert_eq!(sniff(b'G', Mode::Plain), Sniff::Proceed);
        assert_eq!(sniff(0x16, Mode::Tls), Sniff::Proceed);
    }

    #[test]
    fn 离线暂存_入队保序_drain_清空() {
        let mut b = Backlog::new();
        assert!(b.push("trae", "zcode", "任务一".into()).is_none());
        assert!(b.push("trae", "zcode", "任务二".into()).is_none());
        assert_eq!(b.backlog_of("trae"), 2);
        let drained = b.drain("trae");
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].text, "任务一");
        assert_eq!(b.backlog_of("trae"), 0);
    }

    #[test]
    fn 上限淘汰_第201条挤掉最旧() {
        let mut b = Backlog::new();
        for i in 0..BACKLOG_CAP {
            assert!(b.push("trae", "zcode", format!("m{i}")).is_none());
        }
        let evicted = b.push("trae", "zcode", "最新一条".into());
        assert_eq!(evicted.map(|s| s.text), Some("m0".to_string()));
        assert_eq!(b.backlog_of("trae"), BACKLOG_CAP);
        let first = b.drain("trae");
        assert_eq!(first[0].text, "m1");
    }
}
