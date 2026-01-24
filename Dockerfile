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

# ESTA LINHA É CRUCIAL - NÃO PODE FALTAR!
CMD ["/app/server"]
