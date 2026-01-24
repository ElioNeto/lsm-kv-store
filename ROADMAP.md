# Roadmap — LSM KV Store

**Data:** 2026-01-24  
**Modelo base do storage:** `key: String -> value: Vec<u8>` (LSM-Tree)  
**Objetivo:** Evoluir o projeto em versões inteiras, adicionando **iteradores eficientes**, **compaction**, **índices secundários** (posting lists em blocos) e, posteriormente, **múltiplas instâncias** com perfis especializados (Mongo-like e RocksDB/Redis-like).

---

## Convenção de versões

- **Versões sem sufixo** (ex.: v2, v4, v7): entregas evolutivas/experimentais que podem quebrar compatibilidade de API ou formato em disco.
- **Versões `-lts`** (ex.: v3-lts, v5-lts, v6-lts, v8-lts): versões estáveis, prontas para produção, com foco em compatibilidade, migração e operação confiável no "mundo real".

---

## v1 — Status atual (implementado)

### Storage engine

- **MemTable** (BTreeMap) com limite de tamanho configurável (`memtable_max_size`).
- **WAL** (Write-Ahead Log) durável para recuperação de writes não-flushados.
- **Flush** automático para SSTables quando MemTable atinge limite.
- **SSTables** com Bloom Filter para otimizar `get()`.
- **Recovery** do WAL ao inicializar engine.
- **Delete** via tombstone (marcação lógica).
- `stats()` e `stats_all()` para estatísticas do engine.

### Acesso

- **CLI** (REPL) com comandos interativos: `SET`, `GET`, `DELETE`, `SCAN`, `ALL`, `KEYS`, `COUNT`, `STATS`, `BATCH`, `DEMO`.
- **REST API** com endpoints:
  - `GET /health` - healthcheck
  - `GET /stats` e `GET /stats_all` - estatísticas
  - `GET /keys` - listar todas as chaves
  - `GET /keys/{key}` - buscar valor
  - `POST /keys` - inserir/atualizar chave
  - `POST /keys/batch` - inserir múltiplas chaves
  - `DELETE /keys/{key}` - deletar chave
  - `DELETE /keys/batch` - deletar múltiplas chaves
  - `GET /keys/search?q=...&prefix=false` - buscar por substring/prefixo
  - `GET /scan` - retornar todos os dados

### Arquitetura

- **Single-instance**: um único `LsmEngine` por processo, apontando para `./.lsmdata`.
- **Codec básico**: API recebe `value` como `String` e grava `as_bytes().to_vec()`.
- **Busca por prefix/substring**: implementada via `scan()` completo + filtro (não há iteradores eficientes).

### Limitações conhecidas

- ❌ **Sem compaction**: `flush()` contém `TODO compaction`; número de SSTables cresce indefinidamente.
- ❌ **Sem iteradores eficientes**: `search_prefix()` faz scan total.
- ❌ **Sem índices secundários**: queries no value requerem scan total.
- ❌ **Sem multi-instância**: impossível rodar perfis diferentes no mesmo servidor.
- ❌ **Sem codec por instância**: não há suporte para `raw`/`json`/`bson` diferenciados.
- ❌ **Sem validação de integridade**: SSTables corrompidas podem quebrar recovery.

---

## v2 — Base operacional + iteradores (fundação para índices)

### Objetivo

Criar a infraestrutura básica para parar de depender de "scan total" ao buscar por range ou prefixo.

### Entregas

#### Iteradores eficientes no engine

- `iter_prefix(prefix)` e/ou `iter_range(min..max)` que mesclem MemTable + SSTables por ordem de recência, respeitando tombstones.
- Implementação de merge-iterator para combinar múltiplas fontes de dados ordenadas.

#### Otimização de leitura em SSTable

- Introduzir **índice interno** na SSTable (ex.: sparse index com offsets) para evitar varredura linear completa no `get()`.
- Reduzir latência de leitura em SSTables grandes.

#### Robustez

- **Validação de integridade**: checksum por registro ou por bloco.
- **Tolerância a falhas**: ignorar/logar SSTables inválidas durante recovery (não abortar o processo).
- Mensagens de erro mais claras para facilitar debug.

### Critério de pronto

É possível ler chaves `idx:*` por prefixo com paginação estável **sem varrer o banco todo**.

---

## v3-lts — Compaction (sustentar leitura e operação contínua) 🏷️

### Objetivo

Tornar o sistema sustentável para operação contínua, evitando degradação de performance e explosão de SSTables.

### Entregas

#### Compaction inicial

- Implementar estratégia de compaction (sugestão: **size-tiered** ou **leveled**).
- Remover duplicatas (manter versão mais recente de cada chave).
- Remover tombstones definitivamente quando seguro (não há SSTables mais antigas com a chave).
- Controlar número de SSTables ativos.

#### Configuração e tuning

- Parâmetros de compaction configuráveis (ex.: `max_sstables_before_compact`, `compaction_strategy`).
- Logging de operações de compaction para auditoria.

#### Admin básico

- Comando/endpoint para forçar compaction manual (ex.: `POST /admin/compact`).
- Comando/endpoint para verificar integridade (`POST /admin/verify`).

### Critério de pronto

- Número de SSTables estabiliza ao longo do tempo.
- Latência de leitura não degrada continuamente com o volume de writes.
- Sistema opera por dias/semanas sem degradação perceptível.

### Status LTS

✅ **Primeira versão LTS** — KV puro e durável, sem índices avançados, mas já operável para workloads simples de cache, log ou armazenamento de blobs.

---

## v4 — Índices secundários (posting lists em blocos) + Query por índice

### Objetivo

Habilitar **queries no value** sem scan total, usando índices secundários e posting lists em blocos para alto volume.

### Entregas

#### Index Registry

- Arquivo de configuração `indexes.toml` ou `indexes.json` (por instância ou global).
- Define para cada índice:
  - `index_name`
  - `scope_prefix` (opcional, ex.: `users:*`)
  - `index_type` (`equality`, `range`, `text`)
  - `extractor` (como extrair termos do `Vec<u8>`)

#### Extractors (plugins para extrair termos indexáveis)

- `raw`: sem extração (índice direto sobre bytes/string).
- `json_path`: extrai campo JSON via path (ex.: `$.city`).
- `bson_path`: extrai campo BSON via path.
- `custom`: função Rust customizada.

#### Layout de posting lists em blocos

idx:{index}:{term}:meta -> { last_block, total_postings, ... }
idx:{index}:{term}:blk:{000001} -> [pk1, pk2, ...]
idx:{index}:{term}:blk:{000002} -> [pk3, pk4, ...]

#### Atualização de índice no write-path

- **No `SET`**: extrai termos do value (via extractor) e faz append no bloco corrente; cria novo bloco quando cheio.
- **No `DELETE`**: política inicial de **lazy deletion** (marcação lógica); limpeza real em rebuild/compaction.

#### Query API obrigatoriamente indexada

- Endpoint `POST /query` (ou `POST /db/{instance}/query` quando multi-instância estiver pronto).
- Exige parâmetros: `index`, `term` (e opcionalmente `cursor`, `limit`).
- **Sem fallback para scan**: retorna erro se não existir índice compatível.

### Critério de pronto

Query por `city=PortoAlegre` retorna resultados consultando **apenas** `idx:*` + GETs das PKs (sem scan).

---

## v5-lts — Queries compostas + paginação estável + admin de índices 🏷️

### Objetivo

Tornar queries por índice **confiáveis e operáveis em produção**, com suporte a consultas compostas e ferramentas administrativas.

### Entregas

#### Queries compostas

- Suporte a interseção de posting lists (ex.: `city=PortoAlegre AND age=30`).
- Estratégia inicial: carregar blocos do menor conjunto e testar pertença no maior.
- Otimizações futuras: skip pointers, bitsets.

#### Paginação e cursores estáveis

- Cursor como `(term, block_id, offset)` para paginação previsível.
- Garantir que paginação funciona mesmo com writes concorrentes (snapshot read ou versionamento).

#### Limites e proteção

- `limit`: máximo de resultados por request.
- `timeout`: tempo máximo de execução de query.
- `max_postings_scanned`: proteção contra queries explosivas.

#### API administrativa de índices

- `GET /indexes` - listar índices registrados.
- `POST /indexes` - registrar novo índice.
- `DELETE /indexes/{name}` - remover índice.
- `POST /indexes/{name}/rebuild` - reconstruir índice (operação admin; pode ser demorada).

#### Compaction com suporte a índices

- Preservar postings corretos durante compaction.
- Limpar lazy deletions quando possível.
- Oferecer `rebuild index` para corrigir inconsistências.

### Critério de pronto

- Consultas compostas retornam em tempo previsível.
- Paginação estável funciona corretamente.
- Admin consegue criar/remover/reconstruir índices via API.

### Status LTS

✅ **Segunda versão LTS** — KV com índices secundários prontos para produção, adequado para aplicações que precisam query sem scan.

---

## v6-lts — Multi-instância + Codec por instância 🏷️

### Objetivo

Rodar **múltiplas instâncias** no mesmo servidor, cada uma com `data_dir`, tuning e perfil de value independentes (`raw`/`json`/`bson`).

### Entregas

#### Arquivo de configuração `lsm.toml`

```toml
[[instance]]
name = "app"
data_dir = "./.lsm_app"
memtable_max_size = 4194304  # 4MB
codec = "bson"   # ou "json"
query = true
indexes_file = "./indexes_app.toml"

[[instance]]
name = "log"
data_dir = "./.lsm_log"
memtable_max_size = 16777216  # 16MB
codec = "raw"
query = false
indexes_file = "./indexes_log.toml"
Roteamento por instância
POST /db/{instance}/keys

GET /db/{instance}/keys/{key}

POST /db/{instance}/keys/batch

DELETE /db/{instance}/keys/batch

POST /db/{instance}/query

GET /db/{instance}/stats

GET /db/{instance}/indexes

POST /db/{instance}/indexes

etc.

Camada de codec
raw: value é bytes; API pode receber/enviar base64 no HTTP (opcional).

json: API recebe/envia JSON; storage grava UTF-8 bytes.

bson: API recebe/envia JSON; storage grava BSON (melhor preservação de tipos).

Index Registry por instância
indexes_app.toml com extractors JSON/BSON (para instância app).

indexes_log.toml geralmente vazio ou apenas prefix-based (para instância log).

Isolamento completo
Cada instância tem seu próprio LsmEngine, WAL, SSTables, MemTable.

Compaction e recovery são independentes.

Critério de pronto
Conseguir rodar simultaneamente:

Instância app com query=true, codec BSON, e queries indexadas no value.

Instância log como KV puro (query=false), codec raw, para ingestão rápida de logs/counters.

Status LTS
✅ Terceira versão LTS — Multi-instância + codec por instância, pronto para workloads heterogêneos (aplicação + logs/cache) no mesmo servidor.

v7 — Camada "Mongo-like" (coleções/documentos)
Objetivo
Dar ergonomia de MongoDB no acesso, mantendo o motor KV embaixo.

Entregas
Collections/namespace
Convenção de chaves: users:{id}, orders:{id}.

Metadados de collections (opcionalmente armazenados no próprio KV).

Endpoints "Mongo-like"
POST /db/{instance}/collections/{name} - insert document.

GET /db/{instance}/collections/{name}/{id} - findById.

POST /db/{instance}/collections/{name}/find - query indexada (reaproveita posting lists).

PUT /db/{instance}/collections/{name}/{id} - update document.

DELETE /db/{instance}/collections/{name}/{id} - delete document.

Índices declarativos por collection
Configuração de índices por collection usando posting blocks (já existente na v4/v5).

Extrator JSON/BSON automático para campos especificados.

Critério de pronto
Ergonomia de documentos/coleções funcionando sem scan sobre a instância app.

v8-lts — Operação: backup/recovery + ferramentas admin 🏷️
Objetivo
Fornecer ferramentas de operação e manutenção para ambientes de produção.

Entregas
Backup/restore por instância
Snapshot de diretório + manifest (versão, timestamp, SSTables incluídas).

Comando lsm-admin backup {instance} --output backup.tar.gz.

Comando lsm-admin restore {instance} --input backup.tar.gz.

Ferramentas CLI de admin
lsm-admin verify {instance} - verificar integridade de SSTables, WAL, índices.

lsm-admin rebuild-index {instance} {index_name} - reconstruir índice.

lsm-admin compact {instance} - forçar compaction manual.

lsm-admin export {instance} --format json - exportar dados para JSON/CSV.

lsm-admin import {instance} --format json --input data.json - importar dados.

Monitoramento e métricas
Endpoint /metrics (Prometheus-compatible) com estatísticas de cada instância.

Logs estruturados (JSON) para facilitar análise.

Critério de pronto
Processo claro e testado de backup/restore e manutenção repetível por instância.

Status LTS
✅ Quarta versão LTS — Sistema completo de operação, pronto para deploy em produção com suporte a backup, restore e ferramentas de manutenção.

Observações de design (importantes)
Modelo de storage sempre KV: mesmo com "instância Mongo-like", o storage continua key: String -> value: Vec<u8>. A ergonomia de documentos/coleções vem da camada de codec + collections + índices por postings.

Query sem scan: só é viável com índice secundário; posting blocks é a estratégia padrão para alto volume.

Multi-instância: diretórios separados evitam mistura de formatos e facilitam tuning (memtable/compaction) por workload.

Versões LTS: garantem estabilidade de formato em disco e API, com processo de migração documentado entre versões.

Versionamento de formato: a partir de v3-lts, SSTables e WAL devem incluir número de versão de formato para permitir upgrade/downgrade controlado.

Resumo: versões e marcos
Versão	LTS?	Marco principal
v1	❌	KV básico funcional (código atual)
v2	❌	Iteradores eficientes + índice interno em SSTable
v3-lts	✅	Compaction + KV durável para produção
v4	❌	Índices secundários + posting lists
v5-lts	✅	Queries indexadas prontas para produção
v6-lts	✅	Multi-instância + codec por instância
v7	❌	Camada Mongo-like (coleções/documentos)
v8-lts	✅	Backup/restore + ferramentas admin completas
Última atualização: 2026-01-24
Autores: Equipe LSM KV Store
Licença: [definir]
```
