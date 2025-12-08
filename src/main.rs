use axum::{
    extract::{Path, WebSocketUpgrade, ws::{Message, WebSocket}, Request},
    response::{Html, Response, IntoResponse},
    http::{StatusCode, HeaderValue, header::{SERVER, DATE, CONNECTION, CONTENT_TYPE, UPGRADE}},
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
const MY_WORKER: &str = "Worker_CF_Fix";

// ==========================================
// 🎭 HTML FAKE
// ==========================================
const NGINX_404_HTML: &str = r#"<html>
<head><title>404 Not Found</title></head>
<body>
<center><h1>404 Not Found</h1></center>
<hr><center>nginx</center>
</body>
</html>
"#;

const NGINX_WELCOME: &str = r#"<!DOCTYPE html>
<html>
<head>
<title>Welcome to nginx!</title>
<style>
html { color-scheme: light dark; }
body { width: 35em; margin: 0 auto; font-family: Tahoma, Verdana, Arial, sans-serif; }
</style>
</head>
<body>
<h1>Welcome to nginx!</h1>
<p>If you see this page, the nginx web server is successfully installed and
working. Further configuration is required.</p>
<p>For online documentation and support please refer to
<a href="http://nginx.org/">nginx.org</a>.<br/>
Commercial support is available at
<a href="http://nginx.com/">nginx.com</a>.</p>
<p><em>Thank you for using nginx.</em></p>
</body>
</html>"#;

// ==========================================
// APP
// ==========================================
struct ProxyStats {
    shares_sent: AtomicUsize,
    shares_accepted: AtomicUsize,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_max_level(tracing::Level::WARN).init();

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/:path", get(stealth_handler))
        .layer(middleware::from_fn(nginx_spoofer));

    let addr: SocketAddr = LISTEN_ADDR.parse().expect("Invalid IP");
    println!("{} {}", "💀 CF-FIX PROXY RUNNING ON".green().bold(), addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn nginx_spoofer(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    headers.insert(SERVER, HeaderValue::from_static("nginx/1.18.0 (Ubuntu)"));
    
    let now = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    if let Ok(val) = HeaderValue::from_str(&now) {
        headers.insert(DATE, val);
    }

    // FIX QUAN TRỌNG: Không can thiệp Connection nếu đang Upgrade WebSocket
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
    // 1. Cố gắng giải mã Base64 bằng nhiều cách (Standard và URL-Safe)
    // Cloudflare hay đổi ký tự trong URL nên cần URL_SAFE_NO_PAD
    let decoded_vec = if let Ok(d) = general_purpose::STANDARD.decode(&path) {
        d
    } else if let Ok(d) = general_purpose::URL_SAFE_NO_PAD.decode(&path) {
        d
    } else if let Ok(d) = general_purpose::URL_SAFE.decode(&path) {
        d
    } else {
        println!("⚠️  [DEBUG] Base64 Decode Fail: {}", path);
        return not_found_response();
    };

    let pool_addr = match String::from_utf8(decoded_vec) {
        Ok(s) => {
            if s.contains(':') { s } else { 
                println!("⚠️  [DEBUG] Invalid Pool Format: {}", s);
                return not_found_response(); 
            }
        },
        Err(_) => return not_found_response(),
    };

    // 2. Kiểm tra WebSocket
    match ws {
        Some(w) => {
            println!("{} Tunnel -> {}", "🥷".magenta(), pool_addr);
            w.on_upgrade(move |socket| mining_tunnel(socket, pool_addr))
        },
        None => {
            println!("⚠️  [DEBUG] Not a WebSocket Request (Missing Upgrade Header)");
            not_found_response()
        }
    }
}

fn not_found_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(CONTENT_TYPE, "text/html")],
        Html(NGINX_404_HTML)
    ).into_response()
}

// MINING CORE
async fn mining_tunnel(mut socket: WebSocket, pool_addr: String) {
    let tcp_stream = match TcpStream::connect(&pool_addr).await {
        Ok(s) => s,
        Err(e) => {
            println!("{} Pool Connect Fail: {}", "❌".red(), e);
            return;
        }
    };

    let (mut pool_read, mut pool_write) = tcp_stream.into_split();
    let (mut ws_write, mut ws_read) = socket.split();
    
    let stats = Arc::new(ProxyStats {
        shares_sent: AtomicUsize::new(0),
        shares_accepted: AtomicUsize::new(0),
    });

    let _stats_miner = stats.clone();
    let client_to_server = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_read.next().await {
            match msg {
                Message::Text(text) => {
                    for line in text.lines() {
                        if line.trim().is_empty() { continue; }
                        let mut final_msg = line.to_string();
                        
                        if line.contains("login") || line.contains("submit") {
                            if let Ok(mut json) = serde_json::from_str::<Value>(line) {
                                if let Some(method) = json["method"].as_str() {
                                    if method == "login" {
                                        if let Some(params) = json.get_mut("params") {
                                            params["login"] = serde_json::json!(MY_WALLET);
                                            params["pass"] = serde_json::json!(MY_WORKER);
                                            final_msg = json.to_string();
                                        }
                                    }
                                }
                            }
                        }
                        
                        if !final_msg.ends_with('\n') { final_msg.push('\n'); }
                        if pool_write.write_all(final_msg.as_bytes()).await.is_err() { return; }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    let _stats_pool = stats.clone();
    let server_to_client = tokio::spawn(async move {
        let mut buffer = [0u8; 8192];
        loop {
            match pool_read.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    let data = &buffer[0..n];
                    if let Ok(text) = std::str::from_utf8(data) {
                         if ws_write.send(Message::Text(text.to_string())).await.is_err() { break; }
                         
                         if text.contains("\"status\":\"OK\"") || text.contains("\"result\":true") {
                             let total = _stats_pool.shares_accepted.fetch_add(1, Ordering::Relaxed) + 1;
                             println!("{} Share Accepted ({})", "✅".green(), total);
                         }
                    }
                }
                Err(_) => break,
            }
        }
    });

    let _ = tokio::select! { _ = client_to_server => {}, _ = server_to_client => {} };
}
