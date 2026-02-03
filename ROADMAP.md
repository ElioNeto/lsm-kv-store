# Roadmap — LSM KV Store

**Last Updated:** 2026-02-03  
**Base Storage Model:** `key: String -> value: Vec<u8>` (LSM-Tree)  
**Objective:** Evolve the project through versioned releases, adding **efficient iterators**, **compaction**, **secondary indexes** (posting lists in blocks), and eventually **multi-instance** support with specialized profiles (Mongo-like and RocksDB/Redis-like).

---

## Version Convention

- **Regular versions** (e.g., v2, v4, v7): Evolutionary/experimental releases that may break API compatibility or on-disk format.
- **LTS versions** (e.g., v3-lts, v5-lts, v6-lts, v8-lts): Stable versions, production-ready, focused on compatibility, migration, and reliable real-world operation.

---

## v1 — Current Status (Implemented)

### Storage Engine

- **MemTable** (BTreeMap) with configurable size limit (`memtable_max_size`).
- **WAL** (Write-Ahead Log) for durable recovery of unflushed writes.
- **Automatic Flush** to SSTables when MemTable reaches limit.
- **SSTables** with Bloom Filters to optimize `get()` operations.
- **Recovery** from WAL on engine initialization.
- **Delete** via tombstone (logical deletion).
- `stats()` and `stats_all()` for engine statistics.

### Access

- **CLI** (REPL) with interactive commands: `SET`, `GET`, `DELETE`, `SCAN`, `ALL`, `KEYS`, `COUNT`, `STATS`, `BATCH`, `DEMO`.
- **REST API** with endpoints:
  - `GET /health` - Health check
  - `GET /stats` and `GET /stats/all` - Statistics
  - `GET /keys` - List all keys
  - `GET /keys/{key}` - Fetch value
  - `POST /keys` - Insert/update key
  - `POST /keys/batch` - Insert multiple keys
  - `DELETE /keys/{key}` - Delete key
  - `DELETE /keys/batch` - Delete multiple keys
  - `GET /keys/search?q=...&prefix=false` - Search by substring/prefix
  - `GET /scan` - Return all data

### Architecture

- **Single-instance**: One `LsmEngine` per process, pointing to `./.lsmdata`.
- **Basic codec**: API receives `value` as `String` and stores `as_bytes().to_vec()`.
- **Prefix/substring search**: Implemented via full `scan()` + filter (no efficient iterators).

### Known Limitations

- ❌ **No compaction**: `flush()` contains `TODO compaction`; number of SSTables grows indefinitely.
- ❌ **No efficient iterators**: `search_prefix()` performs full scan.
- ❌ **No secondary indexes**: Queries on value require full scan.
- ❌ **No multi-instance**: Impossible to run different profiles on the same server.
- ❌ **No per-instance codec**: No support for differentiated `raw`/`json`/`bson`.
- ❌ **No integrity validation**: Corrupted SSTables can break recovery.

---

## v1.4 — Configuration Refactoring ✅ (Completed - 2026-02-03)

### Objective

Modernize configuration system with centralized, type-safe, and flexible approach.

### Deliverables

#### Centralized Configuration System ✅

- Created unified `LsmConfig` structure in `src/infra/config.rs`
- Separated concerns into:
  - `CoreConfig`: Core engine settings (`dir_path`, `memtable_max_size`)
  - `StorageConfig`: Storage layer settings (`block_size`, `block_cache_size_mb`, `sparse_index_interval`, `bloom_false_positive_rate`)
- Implemented builder pattern via `LsmConfigBuilder`
- Provided sensible defaults for all configuration parameters

#### Code Modernization ✅

- Removed duplicate `LsmConfig` definitions from core modules
- Updated all modules to use centralized configuration
- Removed Portuguese comments for international consistency
- Translated user-facing messages to English

#### Developer Experience ✅

- **Builder Pattern**: Intuitive configuration syntax
  ```rust
  let config = LsmConfig::builder()
      .dir_path("/path/to/data")
      .memtable_max_size(8 * 1024 * 1024)
      .build();
  ```
- **Type Safety**: Strong typing for all parameters
- **Better Defaults**: Sensible defaults reduce boilerplate
- **Backward Compatibility**: Data format unchanged

### Completion Criteria ✅

- All code uses centralized configuration
- Build and tests pass without errors
- Documentation updated (README, CHANGELOG)
- Migration guide provided

---

## v2 — Operational Base + Iterators (Foundation for Indexes)

### Objective

Create infrastructure to stop relying on "full scan" for range or prefix searches.

### Deliverables

#### Efficient Engine Iterators

- `iter_prefix(prefix)` and/or `iter_range(min..max)` that merge MemTable + SSTables by recency order, respecting tombstones.
- Merge-iterator implementation to combine multiple ordered data sources.

#### SSTable Read Optimization

- Introduce **internal index** in SSTable (e.g., sparse index with offsets) to avoid complete linear scan in `get()`.
- Reduce read latency in large SSTables.

#### Robustness

- **Integrity validation**: Checksum per record or per block.
- **Fault tolerance**: Ignore/log invalid SSTables during recovery (don't abort process).
- Clearer error messages for easier debugging.

### Completion Criteria

Possible to read `idx:*` keys by prefix with stable pagination **without scanning the entire database**.

---

## v3-lts — Compaction (Sustain Read and Continuous Operation) 🏷️

### Objective

Make the system sustainable for continuous operation, avoiding performance degradation and SSTable explosion.

### Deliverables

#### Initial Compaction

- Implement compaction strategy (suggestion: **size-tiered** or **leveled**).
- Remove duplicates (keep most recent version of each key).
- Permanently remove tombstones when safe (no older SSTables with the key).
- Control number of active SSTables.

#### Configuration and Tuning

- Configurable compaction parameters (e.g., `max_sstables_before_compact`, `compaction_strategy`).
- Logging of compaction operations for auditing.

#### Basic Admin

- Command/endpoint to force manual compaction (e.g., `POST /admin/compact`).
- Command/endpoint to verify integrity (`POST /admin/verify`).

### Completion Criteria

- Number of SSTables stabilizes over time.
- Read latency doesn't continuously degrade with write volume.
- System operates for days/weeks without noticeable degradation.

### LTS Status

✅ **First LTS version** — Pure and durable KV, without advanced indexes, but already operable for simple cache, log, or blob storage workloads.

---

## v4 — Secondary Indexes (Posting Lists in Blocks) + Index Queries

### Objective

Enable **value queries** without full scan, using secondary indexes and posting lists in blocks for high volume.

### Deliverables

#### Index Registry

- Configuration file `indexes.toml` or `indexes.json` (per instance or global).
- Defines for each index:
  - `index_name`
  - `scope_prefix` (optional, e.g., `users:*`)
  - `index_type` (`equality`, `range`, `text`)
  - `extractor` (how to extract terms from `Vec<u8>`)

#### Extractors (Plugins to Extract Indexable Terms)

- `raw`: No extraction (direct index over bytes/string).
- `json_path`: Extract JSON field via path (e.g., `$.city`).
- `bson_path`: Extract BSON field via path.
- `custom`: Custom Rust function.

#### Posting Lists Layout in Blocks

```
idx:{index}:{term}:meta -> { last_block, total_postings, ... }
idx:{index}:{term}:blk:{000001} -> [pk1, pk2, ...]
idx:{index}:{term}:blk:{000002} -> [pk3, pk4, ...]
```

#### Index Update in Write-Path

- **On `SET`**: Extract terms from value (via extractor) and append to current block; create new block when full.
- **On `DELETE`**: Initial policy of **lazy deletion** (logical marking); actual cleanup in rebuild/compaction.

#### Mandatory Indexed Query API

- Endpoint `POST /query` (or `POST /db/{instance}/query` when multi-instance is ready).
- Requires parameters: `index`, `term` (and optionally `cursor`, `limit`).
- **No scan fallback**: Returns error if compatible index doesn't exist.

### Completion Criteria

Query for `city=PortoAlegre` returns results by consulting **only** `idx:*` + GETs of PKs (no scan).

---

## v5-lts — Composite Queries + Stable Pagination + Index Admin 🏷️

### Objective

Make index queries **reliable and operable in production**, with support for composite queries and administrative tools.

### Deliverables

#### Composite Queries

- Support for posting list intersection (e.g., `city=PortoAlegre AND age=30`).
- Initial strategy: Load blocks from smallest set and test membership in larger.
- Future optimizations: Skip pointers, bitsets.

#### Pagination and Stable Cursors

- Cursor as `(term, block_id, offset)` for predictable pagination.
- Ensure pagination works even with concurrent writes (snapshot read or versioning).

#### Limits and Protection

- `limit`: Maximum results per request.
- `timeout`: Maximum query execution time.
- `max_postings_scanned`: Protection against explosive queries.

#### Index Administrative API

- `GET /indexes` - List registered indexes.
- `POST /indexes` - Register new index.
- `DELETE /indexes/{name}` - Remove index.
- `POST /indexes/{name}/rebuild` - Rebuild index (admin operation; can be time-consuming).

#### Compaction with Index Support

- Preserve correct postings during compaction.
- Clean lazy deletions when possible.
- Offer `rebuild index` to fix inconsistencies.

### Completion Criteria

- Composite queries return in predictable time.
- Stable pagination works correctly.
- Admin can create/remove/rebuild indexes via API.

### LTS Status

✅ **Second LTS version** — KV with production-ready secondary indexes, suitable for applications needing queries without scan.

---

## v6-lts — Multi-Instance + Per-Instance Codec 🏷️

### Objective

Run **multiple instances** on the same server, each with independent `data_dir`, tuning, and value profile (`raw`/`json`/`bson`).

### Deliverables

#### Configuration File `lsm.toml`

```toml
[[instance]]
name = "app"
data_dir = "./.lsm_app"
memtable_max_size = 4194304  # 4MB
codec = "bson"   # or "json"
query = true
indexes_file = "./indexes_app.toml"

[[instance]]
name = "log"
data_dir = "./.lsm_log"
memtable_max_size = 16777216  # 16MB
codec = "raw"
query = false
```

#### Per-Instance Routing

- `POST /db/{instance}/keys`
- `GET /db/{instance}/keys/{key}`
- `POST /db/{instance}/query`
- etc.

#### Codec Layer

- `raw`: Value is bytes; API can receive/send base64 in HTTP (optional).
- `json`: API receives/sends JSON; storage writes UTF-8 bytes.
- `bson`: API receives/sends JSON; storage writes BSON (better type preservation).

#### Complete Isolation

- Each instance has its own LsmEngine, WAL, SSTables, MemTable.
- Compaction and recovery are independent.

### Completion Criteria

Able to run simultaneously:
- `app` instance with `query=true`, BSON codec, and indexed value queries.
- `log` instance as pure KV (`query=false`), raw codec, for fast log/counter ingestion.

### LTS Status

✅ **Third LTS version** — Multi-instance + per-instance codec, ready for heterogeneous workloads (application + logs/cache) on the same server.

---

## v7 — "Mongo-like" Layer (Collections/Documents)

### Objective

Provide MongoDB ergonomics for access, while keeping the KV engine underneath.

### Deliverables

#### Collections/Namespace

- Key convention: `users:{id}`, `orders:{id}`.
- Collection metadata (optionally stored in the KV itself).

#### "Mongo-like" Endpoints

- `POST /db/{instance}/collections/{name}` - Insert document.
- `GET /db/{instance}/collections/{name}/{id}` - FindById.
- `POST /db/{instance}/collections/{name}/find` - Indexed query (reuses posting lists).
- `PUT /db/{instance}/collections/{name}/{id}` - Update document.
- `DELETE /db/{instance}/collections/{name}/{id}` - Delete document.

#### Declarative Indexes per Collection

- Index configuration per collection using posting blocks (from v4/v5).
- Automatic JSON/BSON extractor for specified fields.

### Completion Criteria

Document/collection ergonomics working without scan over `app` instance.

---

## v8-lts — Operations: Backup/Recovery + Admin Tools 🏷️

### Objective

Provide operation and maintenance tools for production environments.

### Deliverables

#### Backup/Restore per Instance

- Directory snapshot + manifest (version, timestamp, included SSTables).
- Command `lsm-admin backup {instance} --output backup.tar.gz`.
- Command `lsm-admin restore {instance} --input backup.tar.gz`.

#### Admin CLI Tools

- `lsm-admin verify {instance}` - Verify integrity of SSTables, WAL, indexes.
- `lsm-admin rebuild-index {instance} {index_name}` - Rebuild index.
- `lsm-admin compact {instance}` - Force manual compaction.
- `lsm-admin export {instance} --format json` - Export data to JSON/CSV.
- `lsm-admin import {instance} --format json --input data.json` - Import data.

#### Monitoring and Metrics

- `/metrics` endpoint (Prometheus-compatible) with statistics for each instance.
- Structured logs (JSON) for easier analysis.

### Completion Criteria

Clear and tested backup/restore process and repeatable per-instance maintenance.

### LTS Status

✅ **Fourth LTS version** — Complete operational system, production-ready with backup, restore, and maintenance tools.

---

## Design Observations (Important)

- **Storage model always KV**: Even with "Mongo-like instance", storage remains `key: String -> value: Vec<u8>`. Document/collection ergonomics come from codec + collections + posting indexes layer.
- **Query without scan**: Only viable with secondary index; posting blocks is the standard strategy for high volume.
- **Multi-instance**: Separate directories avoid format mixing and facilitate per-workload tuning (memtable/compaction).
- **LTS versions**: Guarantee on-disk format and API stability, with documented migration process between versions.
- **Format versioning**: From v3-lts onward, SSTables and WAL must include format version number to allow controlled upgrade/downgrade.

---

## Summary: Versions and Milestones

| Version   | LTS? | Main Milestone                                     |
| :-------- | :--- | :------------------------------------------------- |
| v1        | ❌    | Functional basic KV (current code)                 |
| v1.4      | ✅    | **Configuration refactoring (completed 2026-02-03)** |
| v2        | ❌    | Efficient iterators + internal SSTable index       |
| v3-lts    | ✅    | Compaction + durable KV for production             |
| v4        | ❌    | Secondary indexes + posting lists                  |
| v5-lts    | ✅    | Production-ready indexed queries                   |
| v6-lts    | ✅    | Multi-instance + per-instance codec                |
| v7        | ❌    | Mongo-like layer (collections/documents)           |
| v8-lts    | ✅    | Complete backup/restore + admin tools              |

---

**Last Updated:** 2026-02-03  
**Authors:** LSM KV Store Team  
**License:** MIT
