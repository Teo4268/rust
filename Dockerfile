# ====== Stage 1: Build ======
FROM rust:1.75

WORKDIR /app



# Copy mã nguồn thật
COPY . .

# Build binary thực sự
RUN cargo build --release

# Copy binary từ stage build
COPY --from=builder /app/target/release/rust_proxy /usr/local/bin/rust_proxy


# Run chương trình
CMD ["rust_proxy"]
