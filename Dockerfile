# =========================
# Stage 1: Build
# =========================
FROM rust:latest as builder

WORKDIR /app

# Copy danh sách file Cargo trước
COPY Cargo.toml Cargo.lock ./

# Copy toàn bộ source code
COPY . .

# Build release
RUN cargo build --release

# =========================
# Stage 2: Runtime
# =========================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy đúng binary
COPY --from=builder /app/target/release/rust_proxy /app/rust_proxy

EXPOSE 8080

CMD ["/app/rust_proxy"]
