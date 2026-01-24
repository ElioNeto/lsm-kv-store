# Roadmap — LSM KV Store

Data: 2026-01-23  
Modelo base do storage: **sempre** `key: String -> value: Vec<u8>` (LSM-Tree). [file:42]  
Objetivo: evoluir o projeto em versões, adicionando **índices (posting lists em blocos)** e, na **v3**, suportar **múltiplas instâncias** com “perfil Mongo-like” (JSON/BSON) e “perfil RocksDB/Redis-like” (raw bytes).

---

## v0.1.x — Status atual (concluído)

### Storage engine

- MemTable (BTreeMap), WAL durável, flush para SSTables com Bloom Filter, recovery do WAL. [file:42]
- Delete com tombstone. [file:42]
- `stats()` e testes básicos. [file:42]

### Acesso

- CLI runner (REPL) e REST API (modo recomendado: single-process para evitar MemTables divergentes). [file:42]

---

## v0.2 — Base operacional + iteradores (fundação para índices)

### Objetivo

Criar as bases para operar índices sem depender de “query por scan total”.

### Entregas

- **Iteração eficiente por prefixo/range** no engine:
  - `iter_prefix(prefix)` e/ou `iter_range(min..max)` mesclando MemTable + SSTables por ordem de recência e respeitando tombstones. [file:42]
- SSTable: reduzir custo de leitura:
  - introduzir índice interno (ex.: sparse index/offsets) para evitar varredura linear no `get()`. [file:42]
- Robustez:
  - validação de integridade (checksum/formatos) e tolerância a SSTables inválidas. [file:42]

### Critério de pronto

- É possível ler chaves `idx:*` por prefixo com paginação estável sem varrer o banco todo. [file:42]

---

## v0.3 — Índices secundários (posting lists em blocos) + Query por índice (sem scan)

### Objetivo

Habilitar **queries no value** SEM scan total, usando **índices secundários** e, para alto volume, **posting lists em blocos**. [web:151]

### Entregas

- **Index Registry** (definir “campos indexados”):
  - arquivo `indexes.toml` ou `indexes.json` por instância (ou global, apontando instância/namespace),
  - define: `index_name`, `scope_prefix`, `index_type` (equality/range/text), `extractor`.
- “Extractors” (plugins) para extrair termos indexáveis do `Vec<u8>`:
  - `raw` (sem extração),
  - `json_path` (quando o value for JSON),
  - `bson_path` (quando o value for BSON),
  - `custom` (função Rust).
- Índice por posting blocks (layout proposto):
  - `idx:{index}:{term}:meta -> { last_block, total_postings, ... }`
  - `idx:{index}:{term}:blk:{000001} -> [pk1, pk2, ...]`
  - `idx:{index}:{term}:blk:{000002} -> [...]` [web:151]
- Atualização de índice no write-path:
  - no `SET`: extrai termos e faz append em blocos (cria bloco novo quando cheio). [web:151]
  - no `DELETE`: política inicial de “lazy deletion” (marcações) e limpeza em rebuild/compaction. [web:151]
- **Query API obrigatoriamente indexada**:
  - `POST /query` exige `index` e `term` (e opcionalmente cursor/limit),
  - se não existir índice compatível, retorna erro (sem fallback para scan).

### API administrativa

- `GET /indexes` (listar)
- `POST /indexes` (registrar)
- `DELETE /indexes/{name}` (remover)
- `POST /indexes/{name}/rebuild` (reconstruir índice; operação admin)

### Critério de pronto

- Query por `city=PortoAlegre` retorna resultados consultando apenas `idx:*` + GETs de PK (sem scan). [web:151]

---

## v0.4 — Compaction (para sustentar leitura e índices)

### Objetivo

Evitar degradação e explosão de SSTables; remover duplicatas e tombstones.

### Entregas

- Compaction inicial (ex.: size-tiered) conforme TODO do projeto. [file:42]
- Estratégia para índices durante compaction:
  - preservar postings corretos,
  - limpar lazy deletions quando possível,
  - oferecer `rebuild index` para corrigir inconsistências. [web:151]

### Critério de pronto

- Número de SSTables estabiliza e latência de leitura não degrada continuamente. [file:42]

---

## v0.5 — Queries compostas (sem scan) + paginação/cursores

### Objetivo

Suportar consultas do tipo “A AND B” usando postings.

### Entregas

- Interseção de posting lists (ex.: `city=PortoAlegre AND age=30`):
  - estratégia inicial: carregar blocos do menor conjunto e testar pertença no maior (ou vice-versa),
  - otimizar depois (ordenação, bitsets, skip pointers). [web:151]
- Cursor estável:
  - cursor como `(term, block_id, offset)` para paginação. [web:151]
- Limites e proteção:
  - `limit`, `timeout`, “max postings scanned por request”.

### Critério de pronto

- Consultas compostas retornam em tempo previsível, sem ler base completa.

---

# v3 (v1.0.0) — Múltiplas instâncias com perfis: Mongo-like e RocksDB/Redis-like

> Aqui entra a solução que você pediu: **instância A** “mongo-like” (JSON/BSON + índices/queries) e **instância B** “rocksdb/redis-like” (raw bytes para log/counters), cada uma com diretório próprio.

## v1.0.0 — Multi-instance + Codec por instância (principal)

### Objetivo

Rodar múltiplas instâncias no mesmo servidor, cada uma com:

- `data_dir` independente,
- `memtable_max_size` independente,
- “perfil de value” (codec) independente: `raw` / `json` / `bson`.

### Entregas

- Arquivo de configuração `lsm.toml`:

```toml
[[instance]]
name = "app"
data_dir = "./.lsm_app"
memtable_max_size = 4194304
codec = "bson"   # ou "json"
query = true
indexes_file = "./indexes_app.toml"

[[instance]]
name = "log"
data_dir = "./.lsm_log"
memtable_max_size = 16777216
codec = "raw"
query = false
indexes_file = "./indexes_log.toml"
```

- Server: roteamento por instância:
  - `POST /db/{instance}/keys`
  - `GET /db/{instance}/keys/{key}`
  - `POST /db/{instance}/keys/batch`
  - `DELETE /db/{instance}/keys/batch`
  - `POST /db/{instance}/query`
  - `GET /db/{instance}/stats`
- Camada de codec:
  - `raw`: value é bytes (ou string base64 no HTTP, opcional).
  - `json`: API recebe/envia JSON; storage grava bytes UTF-8.
  - `bson`: API recebe/envia JSON; storage grava BSON (melhor para tipos).
- Index Registry por instância:
  - `indexes_app.toml` com extractors JSON/BSON,
  - `indexes_log.toml` geralmente vazio (ou só prefix-based).

### Critério de pronto

- Você consegue rodar:
  - instância `app` com queries no value via índices,
  - instância `log` como KV puro e rápido para ingestão.

---

## v1.1 — “Mongo-like” (camada de documentos e coleções) — sem mudar o motor

### Objetivo

Dar ergonomia de MongoDB no acesso, mantendo KV no storage.

### Entregas

- Collections/namespace:
  - `users:{id}`, `orders:{id}`.
- Endpoints:
  - `POST /db/app/collections/{name}` (insert)
  - `GET /db/app/collections/{name}/{id}` (findById)
  - `POST /db/app/collections/{name}/find` (query indexada)
- Índices declarativos (por collection) usando posting blocks.

---

## v1.2 — “Redis/RocksDB-like” (log/counters) — features úteis

### Objetivo

Tornar a instância `log` mais próxima do papel de Redis/RocksDB.

### Entregas

- Operações específicas (server-side):
  - `INCR`, `INCRBY` (sobre valores numéricos codificados),
  - `SETNX` (set if not exists),
  - TTL básico (expiração) via marcação + cleanup em compaction (versão inicial).
- (Opcional futuro) Lua scripting para comandos atômicos multi-key (mais próximo do Redis).

---

## v1.3 — Operação: backup/recovery e ferramentas

- Backup/restore por instância (snapshot de diretório + manifest).
- Ferramentas:
  - `lsm-admin verify`
  - `lsm-admin rebuild-index`
  - `lsm-admin compact`
  - `lsm-admin export`

---

## Observações de design (importantes)

- Mesmo com “instância Mongo-like”, o storage continua KV (`Vec<u8>`); o “mongo-like” vem da camada de codec + collections + índices por postings. [web:75][web:151]
- Query no value sem scan só é viável com índice secundário; por isso posting blocks é a estratégia padrão para volume. [web:124][web:151]
- Multi-instance com diretórios separados evita mistura de formatos e facilita tuning (memtable/compaction) por workload.

---
