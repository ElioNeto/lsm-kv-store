# 🦀 LSM KV Store

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](LICENSE)
[![CI Status](https://img.shields.io/github/actions/workflow/status/ElioNeto/lsm-kv-store/rust.yml?style=flat-square&label=build)](https://github.com/ElioNeto/lsm-kv-store/actions)

> **A high-performance, embedded key-value store written in Rust.**

This project is an implementation of the **Log-Structured Merge-Tree (LSM-Tree)** architecture, designed to provide high write throughput with strong durability guarantees. It serves as a study on database internals, storage engines, and system design patterns used in production systems like RocksDB and LevelDB.

---

## 🏗 Architecture

The storage engine follows a standard LSM-Tree architecture with a Write-Ahead Log (WAL) for durability and SSTables for disk storage.

```mermaid
graph TD
    subgraph Client Layer
        CLI[CLI / REPL]
        API[REST API]
    end

    subgraph Storage Engine
        Writer[Write Path]
        Reader[Read Path]
        
        MemTable[MemTable\n(In-Memory BTreeMap)]
        WAL[Write-Ahead Log\n(Disk Append-Only)]
        SSTables[SSTables\n(Sorted String Tables)]
        Bloom[Bloom Filters]
    end

    CLI --> Writer & Reader
    API --> Writer & Reader

    Writer -- 1. Append --> WAL
    Writer -- 2. Insert --> MemTable
    MemTable -- Flush (Threshold Reached) --> SSTables
    
    Reader -- 1. Query --> MemTable
    Reader -- 2. Check Filter --> Bloom
    Bloom -- 3. Scan (If present) --> SSTables

```

### Core Components

| Component | Description |
|-----------|-------------|
| **MemTable** | In-memory write buffer using `BTreeMap`. Provides $O(\log n)$ inserts and ordered iteration. |
| **WAL** | Durable append-only log. Ensures data restoration in case of crash/power loss before a flush. |
| **SSTables** | Immutable, on-disk sorted string tables. Created when MemTable exceeds size limits (4MB default). |
| **Bloom Filters** | Probabilistic data structure attached to each SSTable to prevent expensive disk reads for non-existent keys. |

---

## 🚀 Quick Start

### Prerequisites
- Rust 1.70+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

### Installation & Run

```bash
# Clone and Build
git clone https://github.com/ElioNeto/lsm-kv-store.git
cd lsm-kv-store
cargo build --release

# Run CLI Mode
cargo run --release --bin lsm-kv-store

# Run Server Mode
cargo run --release --bin lsm-server --features api
```

---

## ⚡ Performance & Complexity

This engine is optimized for **write-heavy workloads**.

| Operation | Time Complexity | IO Behavior |
| :--- | :--- | :--- |
| **Write (SET)** | $O(\log N)$ | Memory operation + 1 Append (WAL) |
| **Read (GET)** | $O(K \cdot \log M)$ | Memory lookup + (Potential) Disk Seek per SSTable |
| **Flush** | $O(N)$ | Sequential Write (High throughput) |

> *Note: $N$ = MemTable size, $K$ = Number of SSTables, $M$ = SSTable size.*

### Design Decisions
*   **Why BTreeMap instead of SkipList?**
    *   While RocksDB uses SkipLists for concurrent writes, Rust's standard `BTreeMap` is highly optimized for cache locality and offers excellent single-threaded performance for this v1 implementation.
*   **Why JSON over gRPC?**
    *   For v1, a REST API provides simpler debuggability and easier integration with the provided demo frontend/curl.

---

## 🛠 API & Commands

<details>
<summary><strong>💻 CLI Commands (Click to expand)</strong></summary>

| Command | Description |
|:---|:---|
| `SET <key> <value>` | Insert or update a key-value pair. |
| `GET <key>` | Retrieve value. Returns `Key not found` if missing. |
| `DELETE <key>` | Mark key as deleted (Tombstone). |
| `SCAN <prefix>` | List records starting with prefix. |
| `STATS` | Show internal engine metrics (MemTable size, SST count). |
| `BATCH <n>` | Insert `n` random records for benchmarking. |

</details>

<details>
<summary><strong>🌐 REST API Endpoints (Click to expand)</strong></summary>

Server runs on `http://127.0.0.1:8080`

| Method | Endpoint | Body/Query | Description |
|:---|:---|:---|:---|
| `GET` | `/keys/{key}` | - | Get value for key |
| `POST` | `/keys` | `{"key": "user:1", "value": "data"}` | Insert/Update key |
| `DELETE` | `/keys/{key}` | - | Delete key |
| `GET` | `/keys/search` | `?q=usr&prefix=true` | Search by prefix |
| `GET` | `/stats_all` | - | Full engine telemetry |

</details>

---

## 🗺️ Roadmap

This project is evolving from a concept to a production-hardened engine.

- [x] **v1: Core Engine** (WAL, MemTable, Basic SSTables, Bloom Filters)
- [ ] **v2: Read Optimization** (Sparse Indexing & Block Caching)
- [ ] **v3: Compaction Strategy** (Leveled Compaction to reduce read amplification)
- [ ] **v4: Concurrency** (Lock-free MemTable via Crossbeam/SkipList)

See [ROADMAP.md](ROADMAP.md) for detailed milestones.

---

## 📂 File Structure

```
src/
├── engine.rs      # High-level entry point (facade pattern)
├── memtable.rs    # In-memory storage abstraction
├── wal.rs         # Append-only log management
├── sstable.rs     # Disk storage format & IO
├── codec.rs       # Serialization (Bincode)
└── bin/           # Executables (CLI & Server)
```

## License

MIT License - see [LICENSE](LICENSE) for details.
