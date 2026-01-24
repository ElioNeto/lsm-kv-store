# Build stage com cargo-chef para cache de dependências
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Compilar dependências (será cacheado)
RUN cargo chef cook --release --recipe-path recipe.json
# Compilar aplicação
COPY . .
RUN cargo build --release --bin server

# Runtime stage (imagem final pequena)
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Instalar dependências de runtime
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copiar binário do servidor
COPY --from=builder /app/target/release/server /app/server

# Criar diretório para dados
RUN mkdir -p /data

# Expor porta
EXPOSE 8080

# Executar como root (necessário para Railway volumes)
CMD ["/app/server"]
