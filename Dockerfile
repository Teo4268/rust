FROM rust:1.75 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/rust_proxy /usr/local/bin/rust_proxy
CMD ["rust_proxy"]
