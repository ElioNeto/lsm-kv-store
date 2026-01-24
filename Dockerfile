FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/seu-sgdb /app/sgdb
RUN mkdir -p /data
EXPOSE 8080
CMD ["/app/sgdb"]
