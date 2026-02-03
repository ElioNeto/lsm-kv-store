# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - 2026-02-03

### ♻️ Refactored

#### Configuration Architecture
- **Centralized Configuration System**: Introduced a unified `LsmConfig` structure in `src/infra/config.rs`
  - `CoreConfig`: Manages core engine settings (`dir_path`, `memtable_max_size`)
  - `StorageConfig`: Manages storage layer settings (`block_size`, `block_cache_size_mb`, `sparse_index_interval`, `bloom_false_positive_rate`)
  - Builder pattern implementation via `LsmConfigBuilder` for flexible configuration
  - Default values provided for all configuration parameters

#### Code Modernization
- **Removed duplicate `LsmConfig` definition** from `src/core/engine.rs`
- **Updated all modules** to use centralized configuration from `infra/config.rs`
- **Removed Portuguese comments** throughout the codebase for international consistency
- **Translated user-facing messages** from Portuguese to English in:
  - `src/bin/server.rs`: Server startup messages
  - `examples/demo.rs`: Demo application output

### 🔧 Fixed

#### Engine Layer
- Fixed `SStable::create()` call to include required `StorageConfig` parameter
- Updated config field access patterns:
  - `config.data_dir` → `config.core.dir_path`
  - `config.memtable_max_size` → `config.core.memtable_max_size`

#### Examples
- Updated `examples/basic.rs` to use `LsmConfig::builder()` pattern
- Updated `examples/demo.rs` to use `LsmConfig::builder()` pattern
- Removed hardcoded configuration values in favor of builder pattern

#### Tests
- Updated `tests/restart.rs` to use centralized configuration
- Fixed test config instantiation to use builder pattern
- Updated path access from `cfg.data_dir` to `cfg.core.dir_path`

### 📝 Changed

#### API & Binary
- **Server Binary** (`src/bin/server.rs`):
  - Refactored to use `LsmConfig::builder()` for configuration
  - Environment variables still supported (`DATA_DIR`, `HOST`, `PORT`)
  - Improved error messages and startup logging

#### Developer Experience
- **Builder Pattern**: All configuration now uses intuitive builder syntax:
  ```rust
  let config = LsmConfig::builder()
      .dir_path("/path/to/data")
      .memtable_max_size(8 * 1024 * 1024)
      .block_size(8192)
      .build();
  ```
- **Better Defaults**: Sensible defaults for all configuration options
- **Type Safety**: Strong typing for all configuration parameters

### 🎯 Impact Summary

#### Breaking Changes
- ⚠️ **Configuration Structure Changed**: Direct instantiation of `LsmConfig` is no longer supported
  - **Migration**: Use `LsmConfig::builder()` or `LsmConfig::default()` instead
  - **Old**: `LsmConfig { memtable_max_size: 1024, data_dir: path }`
  - **New**: `LsmConfig::builder().memtable_max_size(1024).dir_path(path).build()`

#### Non-Breaking Changes
- ✅ **API Compatibility**: REST API endpoints remain unchanged
- ✅ **Data Format**: On-disk format (WAL, SSTables) remains compatible
- ✅ **Functionality**: All features continue to work as before

### 📊 Files Changed

#### Core Changes
- `src/core/engine.rs` - Removed duplicate config, updated to use centralized config
- `src/infra/config.rs` - New centralized configuration module
- `src/lib.rs` - Updated exports to include config types

#### Binary & API
- `src/bin/server.rs` - Refactored to use builder pattern
- `src/main.rs` - Updated to use new config structure

#### Examples & Tests
- `examples/basic.rs` - Updated to builder pattern
- `examples/demo.rs` - Updated to builder pattern, translated to English
- `tests/restart.rs` - Updated all test cases to use builder pattern

### 🔄 Commits

1. `3322131` - refactor: remove comments and use centralized config in engine.rs
2. `28380bb` - fix: pass storage config to SStable::create
3. `6307e63` - fix: update server.rs to use centralized config
4. `0f04b1b` - fix: update basic.rs to use centralized config
5. `c5b1a3b` - fix: update demo.rs to use centralized config and remove Portuguese comments
6. `eb54ced` - fix: update restart.rs tests to use centralized config

---

## [1.3.0] - Previous Release

### Added
- Feature Flags system with dynamic management
- REST API with full CRUD operations
- Batch operations support
- Search capabilities (prefix and substring)
- Comprehensive statistics endpoints

### Infrastructure
- SOLID architecture implementation
- Modular design with clear separation of concerns
- Improved error handling
- Bloom Filters for read optimization

---

## Migration Guide

### From main to feature/refactor

If you're upgrading from the main branch, follow these steps:

#### 1. Update Configuration Code

**Before:**
```rust
let config = LsmConfig {
    memtable_max_size: 4 * 1024 * 1024,
    data_dir: PathBuf::from("./.lsm_data"),
};
```

**After:**
```rust
let config = LsmConfig::builder()
    .memtable_max_size(4 * 1024 * 1024)
    .dir_path("./.lsm_data")
    .build();
```

#### 2. Update Field Access

**Before:**
```rust
println!("Data dir: {}", config.data_dir.display());
println!("Max size: {}", config.memtable_max_size);
```

**After:**
```rust
println!("Data dir: {}", config.core.dir_path.display());
println!("Max size: {}", config.core.memtable_max_size);
```

#### 3. Update Imports

Ensure you're importing the config types:
```rust
use lsm_kv_store::{LsmConfig, LsmEngine};
// or for builder specifically:
use lsm_kv_store::{LsmConfig, LsmConfigBuilder, LsmEngine};
```

### No Data Migration Needed

The on-disk format has not changed. Your existing WAL and SSTable files will work without any migration.

---

**Note**: This CHANGELOG covers the refactoring work done in the `feature/refactor` branch compared to `main`.
