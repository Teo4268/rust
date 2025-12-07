# ===== Stage 1: Build =====
FROM rust:latest as builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() { println!(\"placeholder\"); }" > src/main.rs
RUN cargo build --release
RUN rm -rf src

COPY . .
RUN cargo build --release

# ===== Stage 2: Runtime =====
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/* /app/app

EXPOSE 8080

CMD ["/app/app"]
