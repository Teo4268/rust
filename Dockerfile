FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy file rust_proxy đã build sẵn
COPY . .
RUN chmod +x rust_proxy

EXPOSE 8080

CMD ["rust_proxy"]
