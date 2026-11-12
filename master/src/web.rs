//! Web 只读面板：极简 HTTP/1.1 服务（静态页 + JSON API + SSE），无鉴权、全只读。
//!
//! 依赖零外部 HTTP 框架：手写请求头解析 + 固定路由，面件由 `include_str!` 嵌入二进制。

use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::protocol::{now_ms, sniff, Mode, Sniff, State};

const INDEX_HTML: &str = include_str!("../static/index.html");
const STYLE_CSS: &str = include_str!("../static/style.css");
const APP_JS: &str = include_str!("../static/app.js");

// 浏览器图标：透明 PNG 与矢量 logo，随二进制内嵌（零外部文件依赖）。
const LOGO_SVG: &[u8] = include_bytes!("../static/img/logo.svg");
const FAVICON_32: &[u8] = include_bytes!("../static/img/favicon-32.png");
const FAVICON_64: &[u8] = include_bytes!("../static/img/favicon-64.png");
const FAVICON_128: &[u8] = include_bytes!("../static/img/favicon-128.png");
const FAVICON_256: &[u8] = include_bytes!("../static/img/favicon-256.png");
const APPLE_TOUCH_ICON: &[u8] = include_bytes!("../static/img/apple-touch-icon.png");

/// 启动面板监听（TLS 模式复用主节点证书）；监听套接字由调用方绑定（支持端口占用 +1）。
pub async fn serve(
    state: Arc<State>,
    listener: tokio::net::TcpListener,
    addr: String,
    mode: Mode,
    tls: Option<tokio_rustls::TlsAcceptor>,
) {
    eprintln!(
        "[web] 只读面板监听 {}://{}",
        if mode == Mode::Tls { "https" } else { "http" },
        addr
    );
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = state.clone();
                let tls = tls.clone();
                tokio::spawn(async move {
                    let _ = handle(stream, state, mode, tls).await;
                });
            }
            Err(e) => eprintln!("[web] accept 失败: {e}"),
        }
    }
}

async fn handle(
    raw: TcpStream,
    state: Arc<State>,
    mode: Mode,
    tls: Option<tokio_rustls::TlsAcceptor>,
) -> std::io::Result<()> {
    // 嗅探：加密方式不对 → 明文报错（与 hub 同语义）。
    let mut head = [0u8; 1];
    if raw.peek(&mut head).await? == 0 {
        return Ok(());
    }
    if let Sniff::Reject(msg) = sniff(head[0], mode) {
        let mut raw = raw;
        raw.write_all(msg.as_bytes()).await?;
        raw.write_all(b"\r\n").await?;
        raw.flush().await?;
        return raw.shutdown().await;
    }

    match (mode, tls) {
        (Mode::Tls, Some(acc)) => {
            let stream = acc.accept(raw).await.map_err(|e| {
                std::io::Error::other(format!("TLS 握手失败: {e}"))
            })?;
            handle_http(stream, state).await
        }
        _ => handle_http(raw, state).await,
    }
}

async fn handle_http<S>(mut stream: S, state: Arc<State>) -> std::io::Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    // 读请求头（上限 16KB）。
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 2048];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 16 * 1024 {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let mut parts = head.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    if method != "GET" {
        return write_resp(
            &mut stream,
            405,
            "text/plain; charset=utf-8",
            "只读面板仅支持 GET".as_bytes(),
        )
        .await;
    }
    let (path_only, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };

    match path_only {
        "/" | "/index.html" => {
            write_resp(&mut stream, 200, "text/html; charset=utf-8", INDEX_HTML.as_bytes()).await
        }
        "/style.css" => {
            write_resp(&mut stream, 200, "text/css; charset=utf-8", STYLE_CSS.as_bytes()).await
        }
        "/app.js" => {
            write_resp(
                &mut stream,
                200,
                "application/javascript; charset=utf-8",
                APP_JS.as_bytes(),
            )
            .await
        }
        // 浏览器图标（/favicon.ico 兼容：直接回 PNG，现代浏览器可正常识别）。
        "/static/img/logo.svg" | "/favicon.svg" => {
            write_resp(&mut stream, 200, "image/svg+xml", LOGO_SVG).await
        }
        "/static/img/favicon-32.png" | "/favicon.ico" => {
            write_resp(&mut stream, 200, "image/png", FAVICON_32).await
        }
        "/static/img/favicon-64.png" => {
            write_resp(&mut stream, 200, "image/png", FAVICON_64).await
        }
        "/static/img/favicon-128.png" => {
            write_resp(&mut stream, 200, "image/png", FAVICON_128).await
        }
        "/static/img/favicon-256.png" => {
            write_resp(&mut stream, 200, "image/png", FAVICON_256).await
        }
        "/static/img/apple-touch-icon.png" => {
            write_resp(&mut stream, 200, "image/png", APPLE_TOUCH_ICON).await
        }
        "/api/nodes" => {
            let body = nodes_json(&state).await;
            write_json(&mut stream, &body).await
        }
        "/api/messages" => {
            let body = messages_json(&state, query).await;
            write_json(&mut stream, &body).await
        }
        "/api/stats" => {
            let body = stats_json(&state).await;
            write_json(&mut stream, &body).await
        }
        "/events" => sse(stream, state).await,
        _ => write_resp(&mut stream, 404, "text/plain; charset=utf-8", b"not found").await,
    }
}

async fn nodes_json(state: &Arc<State>) -> Value {
    let reg = state.registry.lock().await;
    let backlog = state.backlog.lock().await;
    let mut nodes: Vec<Value> = reg
        .iter()
        .map(|(name, m)| {
            serde_json::json!({
                "name": name,
                "role": m.role,
                "mode": m.mode,
                "online": m.online,
                "internal": m.internal,
                "connected_at_ms": m.connected_at_ms,
                "msgs_out": m.msgs_out,
                "msgs_in": m.msgs_in,
                "backlog": backlog.backlog_of(name),
            })
        })
        .collect();
    nodes.sort_by_key(|v| v["name"].as_str().unwrap_or("").to_string());
    serde_json::json!({ "nodes": nodes })
}

async fn messages_json(state: &Arc<State>, query: Option<&str>) -> Value {
    let mut limit: usize = 200;
    let mut before: u64 = u64::MAX;
    if let Some(q) = query {
        for kv in q.split('&') {
            if let Some((k, v)) = kv.split_once('=') {
                match k {
                    "limit" => limit = v.parse().unwrap_or(200).min(1000),
                    "before" => before = v.parse().unwrap_or(u64::MAX),
                    _ => {}
                }
            }
        }
    }
    let hist = state.history.lock().await;
    let msgs: Vec<Value> = hist
        .records
        .iter()
        .filter(|r| r["seq"].as_u64().unwrap_or(0) < before)
        .rev()
        .take(limit)
        .cloned()
        .collect();
    let total = hist.seq;
    serde_json::json!({ "messages": msgs, "total": total })
}

async fn stats_json(state: &Arc<State>) -> Value {
    let reg = state.registry.lock().await;
    let hist = state.history.lock().await;
    serde_json::json!({
        "online": reg.values().filter(|m| m.online).count(),
        "registered": reg.len(),
        "total_messages": state.msg_total.load(std::sync::atomic::Ordering::Relaxed),
        "history_size": hist.records.len(),
        "uptime_sec": now_ms().saturating_sub(state.started_at_ms) / 1000,
    })
}

/// SSE：实时推送 message / node_join / node_leave 事件。
async fn sse<S>(mut stream: S, state: Arc<State>) -> std::io::Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let header = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Content-Type: text/event-stream; charset=utf-8\r\n",
        "Cache-Control: no-cache\r\n",
        "Connection: keep-alive\r\n",
        "\r\n",
    );
    stream.write_all(header.as_bytes()).await?;

    let mut rx = state.events.subscribe();
    let mut keepalive = tokio::time::interval(std::time::Duration::from_secs(15));
    loop {
        tokio::select! {
            _ = keepalive.tick() => {
                stream.write_all(b": ping\n\n").await?;
            }
            ev = rx.recv() => {
                match ev {
                    Ok(v) => {
                        let line = format!("data: {v}\n\n");
                        stream.write_all(line.as_bytes()).await?;
                    }
                    // 慢消费者：跳过落后事件继续。
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        }
    }
    Ok(())
}

async fn write_json<S>(stream: &mut S, body: &Value) -> std::io::Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    write_resp(
        stream,
        200,
        "application/json; charset=utf-8",
        body.to_string().as_bytes(),
    )
    .await
}

async fn write_resp<S>(
    stream: &mut S,
    status: u16,
    ctype: &str,
    body: &[u8],
) -> std::io::Result<()>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    stream.shutdown().await
}
