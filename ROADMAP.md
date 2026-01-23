# Roadmap atualizado — LSM KV Store (sem mudar o modelo de dados)

Data: 2026-01-23

## Status atual (já desenvolvido)

- Storage engine LSM com **MemTable (BTreeMap)**, **WAL** (append + sync), **flush** para **SSTables** com Bloom filter e recuperação via WAL na inicialização. [file:42]
- API REST e CLI já operam sobre o mesmo `data_dir` e expõem operações básicas (SET/GET/DELETE/SCAN/STATS), com suporte planejado para batch e busca por prefixo/substring. [file:42]

---

## Objetivo desta atualização

Manter o armazenamento exatamente como hoje (key → `Vec<u8>`) e adicionar **queries sobre o value** sem “scan total” sempre, usando **índices secundários** e, para grandes volumes, **posting lists em blocos** (chunked postings). [web:124][web:151]

---

## Milestone 1 — Query no value (MVP por scan)

### Entregas

- Criar um módulo `query` que:
  - faz `scan()` (ou `scan(prefix)` quando existir),
  - decodifica `value` usando um “codec/extractor”,
  - aplica filtros e retorna os resultados. [web:124]
- Esse MVP é o “modo sem índice”: simples, mas com custo proporcional ao tamanho do banco, útil para validar a linguagem de queries e o formato de retorno. [web:124]

### API sugerida

- `POST /query` com algo como:
  - `{ "scope_prefix": "user:", "filter": { "city": "PortoAlegre" }, "limit": 100 }` (sem índice cai em scan). [web:124]

---

## Milestone 2 — Definição de campos indexados (Index Registry)

A ideia é você poder “declarar” quais campos do value geram índice, sem mudar o formato armazenado. [web:124]

### Arquivo de configuração (ex.: `indexes.toml`)

```toml
# indexes.toml

[[index]]
name = "users_city"
scope_prefix = "user:"
type = "equality"
extractor = { kind = "json_path", path = "$.city" }

[[index]]
name = "users_age"
scope_prefix = "user:"
type = "range"
extractor = { kind = "json_path", path = "$.age" }
```

### Como isso funciona

- `scope_prefix` define quais registros entram no índice (ex.: só keys que começam com `user:`). [web:124]
- `extractor` define como extrair o “campo” do `Vec<u8>` (ex.: JSONPath/regex/custom), mantendo o motor KV intacto. [web:124]
- O sistema passa a ter um “Index Registry” carregado na inicialização e usado pelo pipeline de escrita (SET/DELETE). [web:124]

---

## Milestone 3 — Índices secundários com posting lists em blocos (grande volume)

Para queries tipo “city = PortoAlegre”, o índice secundário pode ser visto como `attribute_value -> lista de ponteiros/ids` (postings). [web:124]

### Por que blocos

Guardar **uma lista gigante** em um único value gera alta amplificação de escrita, porque a cada novo item você regrava a lista inteira. [web:151]  
Guardar **um KV por item** reduz isso, mas aumenta overhead e pode tornar compaction pesada em grande escala. [web:151]  
A abordagem de **posting lists em blocos** é o meio-termo: cada “lista” vira vários blocos menores, reduzindo write amplification e mantendo leituras eficientes. [web:151]

### Modelo de chaves do índice (proposta)

Para um índice `users_city` e valor `PortoAlegre`:

- Metadados do termo:
  - `idx:users_city:PortoAlegre:meta -> { "last_block": 12, "len_last": 57 }`
- Blocos (cada bloco é um array de primary keys):
  - `idx:users_city:PortoAlegre:block:000001 -> ["user:1","user:7", ...]`
  - ...
  - `idx:users_city:PortoAlegre:block:000012 -> ["user:991","user:1200", ...]`

### Operações necessárias

- **Index update no SET**:
  - extrair campos indexados do value,
  - atualizar/append no último bloco (se cheio, criar novo bloco). [web:151]
- **Index update no DELETE**:
  - você pode registrar tombstone no índice (lazy delete) e limpar em compaction/rebuild, para evitar “remover do meio da lista” toda hora. [web:151]
- **Reindex** (rebuild) por índice:
  - varrer base (`scan(prefix)`),
  - recomputar postings do zero para garantir consistência quando necessário. [web:124]

### API sugerida

- `POST /indexes/reload` (recarregar `indexes.toml`)
- `POST /indexes/rebuild/{index_name}` (reindex on-demand) [web:124]
- `POST /query`:
  - se o filtro bater com um índice (ex.: equality), usar postings; senão, fallback para scan. [web:124]

---

## Milestone 4 — Range queries (idade, timestamps)

Índices para range (“idade >= 30”) precisam que o valor indexado preserve ordenação para permitir buscas por intervalo. [web:124]

### Opção prática (para começar)

- Normalizar o valor para string ordenável (ex.: zero-pad para inteiros, `age:000030`), e usar chaves:
  - `idx:users_age:000030:user:1 -> ""`
  - `idx:users_age:000031:user:8 -> ""`  
    para permitir varreduras por prefix/range quando o engine tiver iteradores/range scan melhores. [web:124]

---

## Milestone 5 — Engine: iteradores e compaction (para sustentar índices)

Sem compaction, o número de SSTables cresce e tanto queries quanto índices sofrem com leituras mais custosas ao longo do tempo. [file:42]  
Você já deixou “TODO: compaction” no fluxo do flush; o próximo passo é implementar compaction e tombstone cleanup para manter o custo de read/scan sob controle. [file:42]

---

## Ordem recomendada (curta e objetiva)

1. **Query MVP por scan** (define formato de query e retorno). [web:124]
2. **Index Registry** (configurável) + extratores de value. [web:124]
3. **Índice equality com posting blocks** (cidade/status/type). [web:151]
4. **Rebuild de índice** e estratégia de deletes (lazy + compaction). [web:151][web:124]
5. **Compaction** e, depois, índices de range mais completos. [file:42][web:124]

---
