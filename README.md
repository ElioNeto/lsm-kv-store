# LSM-KV-Store

High-performance Key-Value Store using LSM-Tree (Log-Structured Merge-Tree) architecture implemented in Rust.

## Features

- ✅ **LSM-Tree Architecture**: Write-optimized with efficient compaction
- ✅ **Write-Ahead Log (WAL)**: Durability and crash recovery
- ✅ **MemTable**: Fast in-memory writes with configurable size
- ✅ **SSTables**: Sorted String Tables with compression
- ✅ **Bloom Filters**: Fast negative lookups
- ✅ **REST API**: HTTP interface with full CRUD operations
- ✅ **Feature Flags**: Dynamic feature management system
- ✅ **Configurable**: All settings via environment variables

## Quick Start

### Installation

```bash
git clone https://github.com/ElioNeto/lsm-kv-store.git
cd lsm-kv-store
cargo build --release --features api
```

### Configuration

Copy the example environment file and customize:

```bash
cp .env.example .env
```

Edit `.env` to configure:

```bash
# Server
HOST=0.0.0.0
PORT=8080

# Payload Limits (for large datasets/stress tests)
MAX_JSON_PAYLOAD_SIZE=52428800  # 50MB
MAX_RAW_PAYLOAD_SIZE=52428800   # 50MB

# Storage
DATA_DIR=./.lsm_data
MEMTABLE_MAX_SIZE=4194304        # 4MB
BLOCK_SIZE=4096
BLOCK_CACHE_SIZE_MB=64
SPARSE_INDEX_INTERVAL=16
BLOOM_FALSE_POSITIVE_RATE=0.01

# Features
FEATURE_CACHE_TTL=10
```

### Running the Server

```bash
# Using cargo
cargo run --release --features api --bin lsm-server

# Or using the compiled binary
./target/release/lsm-server
```

### Environment Variables Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `HOST` | `0.0.0.0` | Server bind address |
| `PORT` | `8080` | Server port |
| `DATA_DIR` | `./.lsm_data` | Data storage directory |
| `MEMTABLE_MAX_SIZE` | `4194304` (4MB) | MemTable size before flush |
| `MAX_JSON_PAYLOAD_SIZE` | `52428800` (50MB) | Max JSON request/response size |
| `MAX_RAW_PAYLOAD_SIZE` | `52428800` (50MB) | Max raw payload size |
| `BLOCK_SIZE` | `4096` | SSTable block size |
| `BLOCK_CACHE_SIZE_MB` | `64` | Block cache size |
| `SPARSE_INDEX_INTERVAL` | `16` | Blocks between index entries |
| `BLOOM_FALSE_POSITIVE_RATE` | `0.01` | Bloom filter accuracy |
| `FEATURE_CACHE_TTL` | `10` | Feature flags cache TTL (seconds) |

## API Endpoints

### Health Check
```bash
GET /health
```

### Key-Value Operations

```bash
# Set a key
POST /keys
{"key": "user:1", "value": "John Doe"}

# Get a key
GET /keys/{key}

# Delete a key
DELETE /keys/{key}

# List all keys
GET /keys

# Batch insert
POST /keys/batch
{"records": [{"key": "k1", "value": "v1"}, ...]}

# Search keys
GET /keys/search?q=user
GET /keys/search?q=user:&prefix=true

# Scan all
GET /scan
```

### Statistics

```bash
GET /stats      # Basic stats
GET /stats/all  # Detailed stats
```

### Feature Flags

```bash
# List features
GET /features

# Set feature
POST /features/{name}
{"enabled": true, "description": "Feature description"}
```

## Performance Tuning

### For High-Throughput Writes

```bash
MEMTABLE_MAX_SIZE=8388608      # 8MB - flush less frequently
BLOCK_SIZE=8192                # Larger blocks
BLOOM_FALSE_POSITIVE_RATE=0.05 # Less accurate but faster
```

### For Read-Heavy Workloads

```bash
BLOCK_CACHE_SIZE_MB=256        # More cache
BLOOM_FALSE_POSITIVE_RATE=0.001 # More accurate
SPARSE_INDEX_INTERVAL=8        # Denser index
```

### For Stress Testing / Large Datasets

```bash
MAX_JSON_PAYLOAD_SIZE=104857600  # 100MB
MAX_RAW_PAYLOAD_SIZE=104857600   # 100MB
MEMTABLE_MAX_SIZE=16777216       # 16MB
```

## Development

### Run Tests

```bash
cargo test
```

### Run with Debug Logging

```bash
RUST_LOG=debug cargo run --features api --bin lsm-server
```

### Benchmarks

```bash
cargo bench
```

## Architecture

```
┌─────────────┐
│   REST API  │
└──────┬──────┘
       │
┌──────▼──────┐     ┌─────────┐
│  LSM Engine │────▶│   WAL   │
└──────┬──────┘     └─────────┘
       │
   ┌───▼────┐
   │MemTable│
   └───┬────┘
       │ (flush)
   ┌───▼────────┐
   │  SSTables  │ (with Bloom Filters)
   └────────────┘
```

## License

MIT

## Contributing

Contributions are welcome! Please open an issue or PR.
