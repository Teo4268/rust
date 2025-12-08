# ==========================================
# GIAI ĐOẠN 1: BUILDER (Biên dịch Code)
# ==========================================
FROM rust:latest as builder

WORKDIR /app

# 1. Tạo một project giả để cache thư viện (Dependencies)
# Bước này giúp Docker không phải tải lại thư viện mỗi khi bạn sửa code chính
RUN mkdir src && echo "fn main() {}" > src/main.rs
COPY Cargo.toml ./

# 2. Build dependencies trước
RUN cargo build --release

# 3. Bây giờ mới copy source code thật vào
COPY . .

# 4. Đánh dấu file main.rs đã thay đổi để Cargo biên dịch lại code chính
RUN touch src/main.rs

# 5. Build ra file thực thi (Binary) cuối cùng
RUN cargo build --release

# ==========================================
# GIAI ĐOẠN 2: RUNTIME (Môi trường chạy)
# ==========================================
FROM debian:bookworm-slim

# Cài đặt chứng chỉ SSL gốc (Rất quan trọng để kết nối HTTPS/TLS ra Pool)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy file thực thi từ giai đoạn Builder sang
# ⚠️ LƯU Ý: 'black_ops_proxy' phải khớp với tên 'name' trong file Cargo.toml
COPY --from=builder /app/target/release/black_ops_proxy /app/server

# Mở cổng 8080 (Phải khớp với biến LISTEN_ADDR trong main.rs)
EXPOSE 8080

# Chạy server
CMD ["/app/server"]
