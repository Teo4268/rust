# =========================
# Stage 1: Build
# =========================
FROM rust:latest as builder

WORKDIR /app

# Copy Cargo trước để cache dependency
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() { println!(\"placeholder\"); }" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy source thật và build lại
COPY . .
RUN cargo build --release

# =========================
# Stage 2: Runtime
# =========================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Tạo thư mục để copy binary
RUN mkdir -p /app/bin

# Copy đúng NHẤT 1 file binary
COPY --from=builder /app/target/release/rust_proxy /app/bin/rust_proxy

# Chạy file
EXPOSE 8080
CMD ["/app/bin/rust_proxy"]
