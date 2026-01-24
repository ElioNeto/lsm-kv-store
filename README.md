# LSM KV Store

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

High-performance, embedded key-value store written in Rust, based on the **LSM-Tree (Log-Structured Merge-Tree)** architecture. Optimized for high write throughput with durability guarantees via Write-Ahead Log (WAL).

**Current version:** v1 (Development)

---

## Features

### Storage Engine (v1)

- **MemTable**: In-memory write buffer using `BTreeMap` for ordered key storage
- **Write-Ahead Log (WAL)**: Durable append-only log with fsync guarantees
- **SSTables**: Immutable sorted string tables with automatic flush on MemTable overflow
- **Bloom Filters**: Per-SSTable probabilistic filters to reduce unnecessary disk I/O
- **Crash Recovery**: Automatic WAL replay on startup
- **Logical Deletes**: Tombstone markers for efficient delete operations

### Access Methods

- **Interactive CLI**: REPL-style command-line interface for local operations
- **REST API**: HTTP server with JSON endpoints for remote access
- **Library**: Embeddable Rust crate for programmatic usage

---

## Architecture Overview

```

┌─────────────────┐
│   Application   │
└────────┬────────┘
│
┌────┴────┐
│   CLI   │  REST API
└────┬────┘     │
└──────────┤
┌─────▼──────┐
│ LsmEngine  │
└─────┬──────┘
┌──────────┼──────────┐
┌────▼────┐ ┌───▼───┐ ┌───▼────┐
│MemTable │ │  WAL  │ │SSTable │
│(BTreeMap)│ │(.log) │ │ (.sst) │
└─────────┘ └───────┘ └────────┘

```

### Write Path

1. Serialize `LogRecord` (key, value, timestamp, tombstone flag)
2. Append to WAL and sync to disk
3. Insert into MemTable (in-memory BTreeMap)
4. On MemTable size threshold: flush to SSTable, clear WAL

### Read Path

1. Query MemTable (most recent data)
2. If not found, scan SSTables from newest to oldest
3. Use Bloom Filter before reading each SSTable to skip non-existent keys
4. Return first non-tombstone match

---

## Quick Start

### Prerequisites

- Rust 1.70+ ([install via rustup](https://rustup.rs))
- Git

### Installation

```bash
# Clone repository
git clone https://github.com/ElioNeto/lsm-kv-store.git
cd lsm-kv-store

# Build project
cargo build --release

# Run tests
cargo test
```

### Running the CLI

```bash
cargo run --bin lsm-kv-store
```

**Available commands:**

```
SET key value          - Insert or update key-value pair
GET key               - Retrieve value for key
DELETE key            - Mark key as deleted (tombstone)
ALL                   - List all records
KEYS                  - List all keys
COUNT                 - Count active records
STATS                 - Display engine statistics
BATCH n               - Insert n test records
SCAN prefix           - List records by prefix (planned for v2)
DEMO                  - Run automated feature demonstration
HELP                  - Show command reference
EXIT                  - Quit CLI
```

### Running the REST API Server

```bash
cargo run --bin lsm-server --features api
```

Server starts on `http://127.0.0.1:8080`

**Endpoints:**

| Method   | Endpoint                              | Description                                                |
| :------- | :------------------------------------ | :--------------------------------------------------------- |
| `GET`    | `/health`                             | Healthcheck                                                |
| `GET`    | `/stats`                              | Engine statistics (brief)                                  |
| `GET`    | `/stats_all`                          | Detailed statistics (MemTable + SSTables + WAL)            |
| `GET`    | `/keys`                               | List all keys                                              |
| `GET`    | `/keys/{key}`                         | Get value for specific key                                 |
| `POST`   | `/keys`                               | Insert/update key (body: `{"key": "...", "value": "..."}`) |
| `POST`   | `/keys/batch`                         | Batch insert (body: `{"records": [{...}, {...}]}`)         |
| `DELETE` | `/keys/{key}`                         | Delete key (tombstone)                                     |
| `DELETE` | `/keys/batch`                         | Batch delete (body: `{"keys": ["...", "..."]}`)            |
| `GET`    | `/keys/search?q=pattern&prefix=false` | Search by substring or prefix                              |
| `GET`    | `/scan`                               | Full scan (returns all key-value pairs)                    |

**Example requests:**

```bash
# Insert key
curl -X POST http://localhost:8080/keys \
  -H "Content-Type: application/json" \
  -d '{"key": "user:123", "value": "Alice"}'

# Get key
curl http://localhost:8080/keys/user:123

# Search by prefix
curl "http://localhost:8080/keys/search?q=user:&prefix=true"

# Delete key
curl -X DELETE http://localhost:8080/keys/user:123
```

---

## Project Structure

```
lsm-kv-store/
├── src/
│   ├── lib.rs           # Library exports
│   ├── main.rs          # CLI binary
│   ├── engine.rs        # LSM engine core
│   ├── memtable.rs      # In-memory BTreeMap wrapper
│   ├── wal.rs           # Write-Ahead Log
│   ├── sstable.rs       # SSTable read/write
│   ├── log_record.rs    # Record serialization
│   ├── error.rs         # Error types
│   ├── codec.rs         # Binary encoding (bincode)
│   ├── bin/
│   │   └── server.rs    # REST API server
│   └── api.rs           # HTTP handlers (feature-gated)
├── Cargo.toml
├── ROADMAP.md           # Detailed version roadmap
└── README.md
```

**Data directory (default: `./.lsmdata`):**

```
.lsmdata/
├── wal.log              # Write-Ahead Log
├── 1706123456789.sst    # SSTable (timestamp-based naming)
├── 1706123467890.sst
└── ...
```

---

## Configuration

Customize engine behavior via `LsmConfig`:

```rust
use lsm_kv_store::{LsmConfig, LsmEngine};
use std::path::PathBuf;

let config = LsmConfig {
    memtable_max_size: 4 * 1024 * 1024,  // 4MB (default)
    data_dir: PathBuf::from("./data"),
};

let engine = LsmEngine::new(config)?;
```

---

## Performance Characteristics

| Operation   | Complexity                  | Notes                               |
| :---------- | :-------------------------- | :---------------------------------- |
| Write (SET) | O(log n) + O(1) disk append | MemTable insert + WAL append        |
| Delete      | O(log n) + O(1) disk append | Tombstone write                     |
| Read (GET)  | O(log n) + O(k)             | MemTable lookup + k SSTable scans   |
| Flush       | O(n log n)                  | Sort and write n records to SSTable |
| Scan        | O(n × k)                    | Merge n records from k SSTables     |

**Limitations (v1):**

- ⚠️ **No compaction**: SSTable count grows unbounded (planned for v3-lts)
- ⚠️ **Linear SSTable scan**: No internal index (planned for v2)
- ⚠️ **Full scan for prefix search**: No range iterators (planned for v2)

---

## Roadmap

This project follows a versioned roadmap with LTS (Long-Term Support) milestones for production-ready releases.

| Version    | Status     | Focus                                             |
| :--------- | :--------- | :------------------------------------------------ |
| **v1**     | ✅ Current | Basic LSM-Tree KV store with CLI and REST API     |
| v2         | 🔜 Planned | Efficient iterators and SSTable internal indexing |
| **v3-lts** | 🏷️ LTS     | Compaction (first production-ready version)       |
| v4         | 📋 Planned | Secondary indexes with posting lists              |
| **v5-lts** | 🏷️ LTS     | Production-grade indexed queries                  |
| **v6-lts** | 🏷️ LTS     | Multi-instance support with codec per instance    |
| v7         | 📋 Future  | MongoDB-like document/collection layer            |
| **v8-lts** | 🏷️ LTS     | Backup/restore and admin tooling                  |

See [ROADMAP.md](ROADMAP.md) for detailed specifications and release criteria.

---

## Development

### Code Quality Tools

```bash
# Format code
cargo fmt

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Run tests with coverage
cargo test -- --nocapture
```

### Benchmarks

```bash
cargo bench
```

---

## Technical Details

### Data Model

`LogRecord` (serialized via bincode):

```rust
pub struct LogRecord {
    pub key: String,
    pub value: Vec<u8>,
    pub timestamp: u128,    // Nanoseconds since UNIX_EPOCH
    pub is_deleted: bool,   // Tombstone flag
}
```

### SSTable Format

```
┌─────────────────────────────────┐
│ Magic Number (u64)              │  8 bytes
├─────────────────────────────────┤
│ Version (u32)                   │  4 bytes
├─────────────────────────────────┤
│ Bloom Filter Length (u32)       │  4 bytes
│ Bloom Filter Data               │  variable
├─────────────────────────────────┤
│ Metadata Length (u32)           │  4 bytes
│ Metadata (JSON)                 │  variable
├─────────────────────────────────┤
│ Record 1 Length (u32)           │  4 bytes
│ Record 1 Data (bincode)         │  variable
├─────────────────────────────────┤
│ Record 2 Length (u32)           │
│ Record 2 Data                   │
│ ...                             │
└─────────────────────────────────┘
```

---

## Contributing

Contributions are welcome! Priority areas for v1 → v2 transition:

- [ ] Compaction implementation (size-tiered or leveled)
- [ ] SSTable sparse index for faster `get()`
- [ ] Range/prefix iterators (merge-iterator pattern)
- [ ] Checksum validation and corruption handling
- [ ] Crash recovery testing

**Contribution workflow:**

1. Fork repository
2. Create feature branch: `git checkout -b feat/my-feature`
3. Commit changes with clear messages
4. Run tests and linters
5. Open Pull Request with detailed description

---

## License

MIT License - see [LICENSE](LICENSE) for details.

---

## Acknowledgments

Inspired by production LSM-based systems:

- [LevelDB](https://github.com/google/leveldb) (Google)
- [RocksDB](https://github.com/facebook/rocksdb) (Facebook/Meta)
- [Bitcask](https://riak.com/assets/bitcask-intro.pdf) (Riak)

Built with Rust for memory safety and zero-cost abstractions.

---

**Project Status:** Active development (v1)
**Maintainer:** Elio Neto
**Repository:** [github.com/ElioNeto/lsm-kv-store](https://github.com/ElioNeto/lsm-kv-store)
