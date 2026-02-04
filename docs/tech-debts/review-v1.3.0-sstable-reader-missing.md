# Technical Debt: v1.3.0 SSTable Reader Missing Components

**Created**: 2026-02-03  
**PR**: #27  
**Severity**: 🔴 Critical (Blocking)  
**Status**: Open  
**Assigned**: TBD  
**Estimated Effort**: 2 weeks  

---

## 📋 Overview

PR #27 introduces SSTable V2 format with sparse indexing and LZ4 compression but **lacks the Reader implementation** and engine integration necessary to make this feature functional. This creates a significant architectural gap where the system can write V2 SSTables but cannot read them.

### Impact

- ❌ **Production Blocker**: Cannot deploy v1.3.0 without Reader
- ❌ **Dead Code**: Builder implementation is unused by engine
- ❌ **Test Gap**: No integration tests validating round-trip (write → read)
- ❌ **Performance Regression Risk**: Missing Bloom filter checks in read path

### Related Issues

- Issue #19: Task 1.3 - Reader Implementation (incomplete)
- Issue #19: Task 1.4 - Engine Integration (incomplete)
- PR #27: v1.3.0 Release (blocked)

---

## 🎯 Missing Components

### P0 (Critical - Blocking Release)

#### 1. SSTable Reader Implementation

**File**: `src/storage/reader.rs` (does not exist)

**Required Functionality**:
```rust
pub struct SstableReader {
    metadata: MetaBlock,
    bloom_filter: Bloom<[u8]>,
    sparse_index: Vec<BlockMeta>,
    file: File,
    block_cache: LruCache<u64, Vec<u8>>,
    path: PathBuf,
}

impl SstableReader {
    /// Open an SSTable V2 file for reading
    pub fn open(path: PathBuf, cache_size_mb: usize) -> Result<Self>;
    
    /// Retrieve a value by key using sparse index and Bloom filter
    pub fn get(&mut self, key: &str) -> Result<Option<LogRecord>>;
    
    /// Scan all records in the SSTable (for compaction)
    pub fn scan(&mut self) -> Result<impl Iterator<Item = Result<LogRecord>>>;
    
    /// Check if key might exist (Bloom filter)
    pub fn might_contain(&self, key: &str) -> bool;
    
    // Private methods
    fn read_footer(&mut self) -> Result<u64>;  // Returns meta offset
    fn read_meta_block(&mut self, offset: u64) -> Result<MetaBlock>;
    fn read_and_decompress_block(&mut self, block_meta: &BlockMeta) -> Result<Vec<u8>>;
    fn binary_search_block(&self, key: &str) -> Option<&BlockMeta>;
}
```

**Implementation Steps**:
1. Parse footer (last 8 bytes) to get metadata offset
2. Read and decompress metadata block
3. Deserialize sparse index and Bloom filter
4. Implement `get()` with:
   - Bloom filter pre-check (early exit if definitely not present)
   - Binary search on sparse index using `partition_point`
   - Read and decompress target block
   - Linear scan within block
5. Implement LRU block cache (use `lru` crate)
6. Add comprehensive error handling for corrupted files

**Acceptance Criteria**:
- [ ] Successfully reads SSTables created by Builder
- [ ] Bloom filter reduces unnecessary disk I/O
- [ ] Block cache improves repeated read performance
- [ ] Handles corrupted files gracefully (validation errors)
- [ ] All edge cases tested (empty blocks, boundary keys, etc.)

**Estimated Effort**: 3-5 days

---

#### 2. Engine Integration

**Files to Update**:
- `src/core/engine.rs`
- `src/storage/mod.rs`
- `src/storage/sstable.rs` (if exists)

**Required Changes**:

##### 2.1 Update Flush Logic
```rust
// OLD (v1.0):
let sstable = SStable::create(&path, &records)?;

// NEW (v1.3):
let mut builder = SstableBuilder::new(path, self.config.storage.clone(), timestamp)?;
for (key, record) in sorted_records {
    builder.add(key.as_bytes(), &record)?;
}
let sstable_path = builder.finish()?;
let reader = SstableReader::open(sstable_path, self.config.storage.block_cache_size_mb)?;
self.sstables.push(reader);
```

##### 2.2 Update Read Path
```rust
pub fn get(&mut self, key: &str) -> Result<Option<Vec<u8>>> {
    // 1. Check MemTable (unchanged)
    if let Some(record) = self.memtable.get(key) {
        return Ok(record.value.clone());
    }
    
    // 2. Check SSTables (newest first) with Bloom filter optimization
    for sstable in self.sstables.iter_mut() {
        // NEW: Bloom filter check (skip entire SSTable if key definitely not present)
        if !sstable.might_contain(key) {
            continue;
        }
        
        if let Some(record) = sstable.get(key)? {
            return Ok(Some(record.value));
        }
    }
    
    Ok(None)
}
```

##### 2.3 Format Migration Strategy
```rust
pub enum SstableVersion {
    V1(SStableV1),  // Old format (backward compat)
    V2(SstableReader),  // New format
}

impl SstableVersion {
    pub fn open(path: PathBuf) -> Result<Self> {
        let mut file = File::open(&path)?;
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        
        match &magic {
            b"LSMSST01" => Ok(Self::V1(SStableV1::load(&path)?)),
            b"LSMSST02" => Ok(Self::V2(SstableReader::open(path)?)),
            _ => Err(LsmError::InvalidSstableFormat),
        }
    }
    
    pub fn get(&mut self, key: &str) -> Result<Option<LogRecord>> {
        match self {
            Self::V1(v1) => v1.get(key),
            Self::V2(v2) => v2.get(key),
        }
    }
}
```

**Acceptance Criteria**:
- [ ] Engine uses Builder for new SSTables
- [ ] Engine reads both V1 and V2 formats
- [ ] Bloom filter optimization active in read path
- [ ] Performance benchmarks show improvement over V1
- [ ] Migration path documented

**Estimated Effort**: 2-3 days

---

#### 3. Configuration Validation

**File**: `src/api/config.rs`

**Add Validation Method**:
```rust
impl ServerConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        // Port validation
        if self.port == 0 {
            return Err(ConfigError::InvalidPort("Port cannot be 0".into()));
        }
        
        // Payload size validation
        if self.max_json_payload_size == 0 {
            return Err(ConfigError::InvalidPayloadSize(
                "Payload size must be greater than 0".into()
            ));
        }
        
        if self.max_json_payload_size > 1024 * 1024 * 1024 {  // 1GB
            return Err(ConfigError::InvalidPayloadSize(
                "Payload size cannot exceed 1GB".into()
            ));
        }
        
        // Feature cache TTL validation
        if self.feature_cache_ttl_secs == 0 {
            return Err(ConfigError::InvalidCacheTtl(
                "Cache TTL must be greater than 0".into()
            ));
        }
        
        Ok(())
    }
    
    pub fn from_env() -> Result<Self, ConfigError> {
        let config = Self::from_env_unchecked();
        config.validate()?;  // Fail fast on invalid config
        Ok(config)
    }
    
    fn from_env_unchecked() -> Self {
        // Current implementation (renamed)
    }
}
```

**Add Storage Config Validation**:
```rust
impl StorageConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.block_size == 0 {
            return Err(ConfigError::InvalidBlockSize("Block size cannot be 0".into()));
        }
        
        if self.block_size > 1024 * 1024 {  // 1MB
            return Err(ConfigError::InvalidBlockSize(
                "Block size cannot exceed 1MB (consider smaller blocks)".into()
            ));
        }
        
        if self.sparse_index_interval == 0 {
            return Err(ConfigError::InvalidIndexInterval(
                "Sparse index interval cannot be 0".into()
            ));
        }
        
        if self.bloom_false_positive_rate <= 0.0 || self.bloom_false_positive_rate >= 1.0 {
            return Err(ConfigError::InvalidBloomRate(
                "Bloom FP rate must be between 0 and 1".into()
            ));
        }
        
        // Cross-parameter validation
        if self.sparse_index_interval > 1000 {
            eprintln!("⚠️  Warning: Very sparse index (interval={}), may impact read performance", 
                     self.sparse_index_interval);
        }
        
        Ok(())
    }
}
```

**Acceptance Criteria**:
- [ ] Invalid configs fail fast on startup with clear errors
- [ ] All parameters have bounds checking
- [ ] Cross-parameter validation (e.g., interval vs block size)
- [ ] Unit tests for all validation scenarios

**Estimated Effort**: 1 day

---

#### 4. Block Caching Implementation

**Dependencies**: Add to `Cargo.toml`:
```toml
[dependencies]
lru = "0.12"
```

**Implementation in Reader**:
```rust
use lru::LruCache;
use std::num::NonZeroUsize;

pub struct SstableReader {
    // ... existing fields ...
    block_cache: LruCache<u64, Vec<u8>>,  // offset -> decompressed data
}

impl SstableReader {
    pub fn open(path: PathBuf, cache_size_mb: usize) -> Result<Self> {
        // Calculate cache capacity in blocks
        let avg_block_size = 4096;  // 4KB default
        let cache_size_bytes = cache_size_mb * 1024 * 1024;
        let cache_capacity = NonZeroUsize::new(cache_size_bytes / avg_block_size)
            .unwrap_or(NonZeroUsize::new(100).unwrap());
        
        let block_cache = LruCache::new(cache_capacity);
        
        // ... rest of initialization ...
        
        Ok(Self {
            block_cache,
            // ...
        })
    }
    
    fn read_block(&mut self, block_meta: &BlockMeta) -> Result<Vec<u8>> {
        // Check cache first
        if let Some(cached) = self.block_cache.get(&block_meta.offset) {
            return Ok(cached.clone());
        }
        
        // Cache miss - read from disk
        let block_data = self.read_and_decompress_block(block_meta)?;
        
        // Store in cache
        self.block_cache.put(block_meta.offset, block_data.clone());
        
        Ok(block_data)
    }
}
```

**Metrics to Track**:
- Cache hit rate (hits / total reads)
- Average decompression time saved
- Memory usage

**Acceptance Criteria**:
- [ ] LRU cache implemented with configurable size
- [ ] Cache hit rate > 70% for hot data workloads
- [ ] Memory usage stays within `BLOCK_CACHE_SIZE_MB` limit
- [ ] Benchmarks show performance improvement

**Estimated Effort**: 1-2 days

---

### P1 (High Priority - Should Have)

#### 5. Comprehensive Test Suite

**New Test Files**:
- `tests/integration/sstable_v2_roundtrip.rs`
- `tests/integration/config_validation.rs`
- `tests/unit/reader_edge_cases.rs`

**Critical Tests to Add**:

```rust
// Integration test: Full round-trip
#[test]
fn test_sstable_v2_write_read_roundtrip() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("roundtrip.sst");
    let config = StorageConfig::default();
    
    // Write 1000 records
    let mut builder = SstableBuilder::new(path.clone(), config.clone(), 123)?;
    let test_data: Vec<_> = (0..1000)
        .map(|i| (format!("key_{:04}", i), format!("value_{:04}", i)))
        .collect();
    
    for (key, value) in &test_data {
        builder.add(key.as_bytes(), &create_test_record(key, value.as_bytes()))?;
    }
    builder.finish()?;
    
    // Read and verify
    let mut reader = SstableReader::open(path, config.block_cache_size_mb)?;
    
    for (key, expected_value) in &test_data {
        let record = reader.get(key)?.expect("Key should exist");
        assert_eq!(&record.value, expected_value.as_bytes());
    }
    
    // Verify non-existent keys
    assert!(reader.get("missing_key")?.is_none());
    
    Ok(())
}

// Edge case: Exact boundary keys
#[test]
fn test_reader_boundary_keys() -> Result<()> {
    // Test first_key, last_key, key between blocks
    // ...
}

// Edge case: Corrupted data
#[test]
fn test_reader_corrupted_footer() -> Result<()> {
    // Corrupt last 8 bytes and verify graceful error
    // ...
}

// Configuration validation
#[test]
fn test_config_validation_invalid_port() {
    env::set_var("PORT", "0");
    let result = ServerConfig::from_env();
    assert!(matches!(result, Err(ConfigError::InvalidPort(_))));
}

// Bloom filter effectiveness
#[test]
fn test_bloom_filter_false_positives() -> Result<()> {
    // Verify FP rate is within configured bounds
    // ...
}

// Block cache effectiveness
#[test]
fn test_block_cache_hit_rate() -> Result<()> {
    // Read same keys multiple times, verify cache hits
    // ...
}
```

**Acceptance Criteria**:
- [ ] Integration tests cover write → read → verify
- [ ] Edge cases tested (boundaries, corruption, empty)
- [ ] Configuration validation has 100% coverage
- [ ] Performance benchmarks included
- [ ] Test coverage > 85% for new code

**Estimated Effort**: 2-3 days

---

#### 6. Performance Benchmarks

**File**: `benches/sstable_v2_performance.rs`

**Benchmarks to Add**:
```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn benchmark_read_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("sstable_read");
    
    // Compare V1 vs V2 format
    for format in ["v1", "v2"].iter() {
        for num_keys in [1000, 10000, 100000].iter() {
            group.bench_with_input(
                BenchmarkId::new(*format, num_keys),
                num_keys,
                |b, &size| {
                    let reader = setup_sstable(*format, size);
                    b.iter(|| {
                        reader.get(&random_key());
                    });
                },
            );
        }
    }
    
    group.finish();
}

fn benchmark_bloom_filter_effectiveness(c: &mut Criterion) {
    // Measure how often Bloom filter saves disk I/O
}

fn benchmark_cache_hit_rate(c: &mut Criterion) {
    // Measure cache effectiveness for hot keys
}

fn benchmark_compression_ratio(c: &mut Criterion) {
    // Measure LZ4 compression ratio and speed
}

criterion_group!(
    benches,
    benchmark_read_performance,
    benchmark_bloom_filter_effectiveness,
    benchmark_cache_hit_rate,
    benchmark_compression_ratio
);
criterion_main!(benches);
```

**Expected Results**:
- V2 read latency: < 1ms for hot data (cache hit)
- V2 read latency: < 10ms for cold data (disk read + decompress)
- Bloom filter FP rate: < 1% (matches configuration)
- Cache hit rate: > 70% for hot workloads
- Compression ratio: 2-4x space savings

**Estimated Effort**: 1-2 days

---

#### 7. Enhanced Error Handling

**Create Custom Error Types**:
```rust
#[derive(Debug, thiserror::Error)]
pub enum SstableError {
    #[error("Invalid SSTable format: expected {expected:?}, found {found:?}")]
    InvalidMagic {
        expected: Vec<u8>,
        found: Vec<u8>,
    },
    
    #[error("Corrupted footer: could not parse metadata offset")]
    CorruptedFooter,
    
    #[error("Block too large: {size} bytes (max: {max})")]
    BlockTooLarge { size: usize, max: usize },
    
    #[error("Entry too large for block: key='{key}', entry_size={entry_size}, max_block_size={max_block_size}")]
    EntryTooLarge {
        key: String,
        entry_size: usize,
        max_block_size: usize,
    },
    
    #[error("Decompression failed for block at offset {offset}: {source}")]
    DecompressionFailed {
        offset: u64,
        #[source]
        source: std::io::Error,
    },
    
    #[error("Sparse index corrupted: {0}")]
    CorruptedIndex(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

**Usage in Code**:
```rust
// Before:
return Err(LsmError::CompactionFailed("Entry too large".to_string()));

// After:
return Err(SstableError::EntryTooLarge {
    key: String::from_utf8_lossy(key).to_string(),
    entry_size: value_bytes.len(),
    max_block_size: self.config.block_size,
}.into());
```

**Estimated Effort**: 1 day

---

### P2 (Medium Priority - Nice to Have)

#### 8. Compression Optimizations

**Small Block Skip**:
```rust
const MIN_COMPRESSION_SIZE: usize = 512;  // Don't compress < 512 bytes

fn compress_block(&self, data: &[u8]) -> (Vec<u8>, bool) {
    if data.len() < MIN_COMPRESSION_SIZE {
        return (data.to_vec(), false);  // Uncompressed
    }
    
    let compressed = compress_prepend_size(data);
    
    // Check if compression helped
    if compressed.len() >= data.len() {
        (data.to_vec(), false)  // Use uncompressed
    } else {
        (compressed, true)  // Use compressed
    }
}
```

**Update BlockMeta**:
```rust
pub struct BlockMeta {
    pub first_key: Vec<u8>,
    pub offset: u64,
    pub size: u32,
    pub uncompressed_size: u32,
    pub is_compressed: bool,  // NEW
}
```

**Estimated Effort**: 1 day

---

#### 9. Per-Block Bloom Filters

**Optimization**: Add Bloom filter per block to skip decompression

```rust
pub struct BlockMeta {
    pub first_key: Vec<u8>,
    pub offset: u64,
    pub size: u32,
    pub uncompressed_size: u32,
    pub bloom_filter: Vec<u8>,  // NEW: 100-500 bytes per block
}
```

**Benefit**: Save CPU cycles on decompression when key definitely not in block

**Estimated Effort**: 2 days

---

## 📊 Tracking Progress

### Completion Checklist

#### P0 Items (Must Complete)
- [ ] 1. SSTable Reader Implementation (0%)
- [ ] 2. Engine Integration (0%)
- [ ] 3. Configuration Validation (0%)
- [ ] 4. Block Caching (0%)

#### P1 Items (Should Complete)
- [ ] 5. Comprehensive Test Suite (20% - basic tests exist)
- [ ] 6. Performance Benchmarks (0%)
- [ ] 7. Enhanced Error Handling (30% - partial)

#### P2 Items (Nice to Have)
- [ ] 8. Compression Optimizations (0%)
- [ ] 9. Per-Block Bloom Filters (0%)

### Overall Progress: **5% Complete**

---

## 🎯 Acceptance Criteria for Debt Resolution

### Functional Requirements
- [x] SSTable Builder creates V2 format files
- [ ] SSTable Reader can open and read V2 format files
- [ ] Engine uses Builder for flush operations
- [ ] Engine uses Reader for get operations
- [ ] Bloom filters reduce unnecessary disk I/O
- [ ] Block cache improves repeated read performance
- [ ] Configuration validation prevents invalid startup states

### Quality Requirements
- [ ] Test coverage > 85% for new code
- [ ] All integration tests pass (write → read → verify)
- [ ] Performance benchmarks show improvement over V1
- [ ] Zero clippy warnings
- [ ] Zero compilation warnings
- [ ] Documentation updated (module docs, comments, guides)

### Performance Requirements
- [ ] Read latency < 1ms for cache hits
- [ ] Read latency < 10ms for cache misses
- [ ] Bloom filter false positive rate matches configuration
- [ ] Cache hit rate > 70% for hot workloads
- [ ] Compression ratio: 2-4x space savings

---

## 📝 Resolution Plan

### Option A: Complete in Current PR (Recommended)

**Timeline**: 2 weeks

1. **Week 1**: Implement Reader + Engine Integration
   - Days 1-3: Reader implementation
   - Days 4-5: Engine integration

2. **Week 2**: Testing + Validation + Benchmarks
   - Days 1-2: Integration tests + edge cases
   - Day 3: Configuration validation
   - Days 4-5: Performance benchmarks + optimization

**Pros**: Ships complete feature, no partial states
**Cons**: Delays v1.3.0 release by 2 weeks

### Option B: Split into Multiple PRs

**Timeline**: 2-3 weeks (parallel work possible)

1. **PR #27A**: Configuration System Only
   - Merge immediately (independent feature)
   - No blocking dependencies

2. **PR #27B**: SSTable Builder
   - Keep as-is but mark as "infrastructure only"
   - Blocked until PR #27C

3. **PR #27C**: SSTable Reader + Engine Integration
   - Completes the feature
   - Must include all integration tests
   - Unblocks PR #27B

4. **PR #27**: Release Preparation
   - Merge after A, B, C complete
   - Final integration tests
   - Update to v1.3.0

**Pros**: Allows incremental progress, parallel work
**Cons**: More complex coordination, risk of merge conflicts

---

## 🔗 References

- **PR #27**: https://github.com/ElioNeto/lsm-kv-store/pull/27
- **Issue #19**: Task 1.3 - Reader and Integration
- **ROADMAP.md**: v1.3.0 milestone tracking
- **CHANGELOG.md**: v1.3.0 release notes (premature)
- **Review Comment**: https://github.com/ElioNeto/lsm-kv-store/pull/27#issuecomment-3843773961

---

## 👥 Stakeholders

- **Owner**: @ElioNeto
- **Reviewer**: Tech Lead (Senior Rust Expert)
- **Priority**: P0 (Critical - Blocking v1.3.0 release)

---

## 📅 Timeline

| Date | Milestone |
|------|----------|
| 2026-02-03 | Technical debt identified and documented |
| 2026-02-10 | Target: Reader implementation complete |
| 2026-02-14 | Target: Engine integration complete |
| 2026-02-17 | Target: All tests + benchmarks complete |
| 2026-02-18 | Target: v1.3.0 ready for merge |

---

**Status**: 🔴 Open - Awaiting resolution  
**Next Action**: Team decision on Option A vs Option B  
**Blocked By**: None (can start immediately)  
**Blocking**: PR #27 merge, v1.3.0 release
