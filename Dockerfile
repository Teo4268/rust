# ====== Stage 1: Build ======
FROM rust:1.75 AS builder

WORKDIR /app

# Copy file Cargo.toml và Cargo.lock trước để cache dependency
COPY Cargo.toml Cargo.lock ./

# Tạo dummy main để cache build dependency (tăng tốc build lần sau)
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy mã nguồn thật
COPY . .

# Build binary thực sự
RUN cargo build --release

# ====== Stage 2: Runtime (nhỏ gọn) ======
FROM debian:bookworm-slim

# Cài thư viện cần thiết (tùy framework)
RUN apt-get update && apt-get install -y \
    ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary từ stage build
COPY --from=builder /app/target/release/rust_proxy /usr/local/bin/rust_proxy

# Expose port nếu service cần
EXPOSE 8080

# Run chương trình
CMD ["rust_proxy"]
