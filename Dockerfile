# ===== Stage 1: Build =====
FROM rust:1.75 as builder

WORKDIR /app

# Copy file cargo để cache dependency
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() { println!(\"placeholder\"); }" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy toàn bộ source code thật và build lại
COPY . .
RUN cargo build --release

# ===== Stage 2: Runtime =====
FROM debian:bookworm-slim

# Cài thư viện cần thiết (tùy dự án)
RUN apt-get update && apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary từ stage builder sang
COPY --from=builder /app/target/release/* /app/app

# Expose port (tùy project)
EXPOSE 8080

# Run
CMD ["/app/app"]
