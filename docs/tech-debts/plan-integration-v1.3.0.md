# Implementation Plan: V2 Integration and Bug Fixes

**Date**: 2026-02-04
**Target Release**: v1.3.1
**Related Issues**: #38 (Integration), #39 (Block Overflow)

---

## 1. Overview

This document outlines the technical approach to integrate the new SSTable V2 implementation into the `LsmEngine` and fix the critical block offset overflow bug. These changes are necessary to resolve technical debt and ensure data stability for blocks larger than 64KB.

## 2. Bug Fix: Block Offset Overflow (#39)

### Problem
The `Block` struct uses `u16` for offsets. If `block_size` > 65,535 bytes (64KB), the offset truncates, causing corruption. `StorageConfig` allows up to 1MB blocks.

### Solution Plan
1.  **Refactor `Block` Struct**:
    - Change `offsets: Vec<u16>` to `offsets: Vec<u32>`.
    - Change internal length tracking for keys/values to `u32` where necessary during encoding.
2.  **Update Encoding Format**:
    - Protocol change: Write 4 bytes (u32) for offsets and element counts instead of 2 bytes.
    - **Note**: This breaks compatibility with existing V2 blocks (magic `LSMSST02`). Since V2 is not yet live in production (engine integration pending), we can break this format without migration scripts, just by bumping the magic to `LSMSST03` or resetting test data.
3.  **Validation**:
    - Add test case: `test_block_overflow_u16` with a block size of 70KB.

---

## 3. Integration Plan: Replace V1 with V2 (#38)

### Phase 1: Engine Refactoring
The `LsmEngine` currently holds `Vec<SStable>` (V1).

1.  **Dependency Swap**:
    - In `src/core/engine.rs`:
        - Remove usage of `crate::storage::sstable::SStable`.
        - Import `crate::storage::reader::SstableReader`.
        - Import `crate::storage::builder::SstableBuilder`.
2.  **Struct Update**:
    - Change `sstables` field type:
      ```rust
      pub struct LsmEngine {
          // ...
          pub(crate) sstables: Mutex<Vec<SstableReader>>, // Was Vec<SStable>
      }
      ```
3.  **Refactor `open/recover`**:
    - In `LsmEngine::new()`:
        - When iterating directory files, use `SstableReader::open(path, config)`.
        - **Format Check**: If `SstableReader::open` fails due to magic number mismatch (old V1 files), log a warning and skip/archive them (or implement a converter). *For this sprint, we assume a clean slate or data reset.*

### Phase 2: Write Path (`flush`)
1.  **Update `flush()`**:
    - Replace `SStable::create(...)` with `SstableBuilder`.
    ```rust
    // Logic:
    let mut builder = SstableBuilder::new(path, config, timestamp)?;
    for record in records {
        builder.add(&key, &record)?;
    }
    let sst_path = builder.finish()?;
    let reader = SstableReader::open(sst_path, config)?;
    self.sstables.lock().unwrap().insert(0, reader);
    ```

### Phase 3: Read Path (`get/scan`)
1.  **Update `get()`**:
    - `SstableReader::get()` returns `Result<Option<LogRecord>>`. The logic remains similar, but the underlying implementation now uses the sparse index + Bloom filter correctly.
2.  **Update `scan()`**:
    - `SstableReader` implements `scan()`. Update `LsmEngine::scan()` to iterate over readers using this method.

---

## 4. Execution Order

1.  **Fix #39 first**: Ensure the Block format is stable and supports large files before wiring it into the engine.
2.  **Execute #38**: Perform the engine surgery.
3.  **Cleanup**: Delete `src/storage/sstable/mod.rs` (Legacy V1).

## 5. Verification
- Run `cargo test` (ensuring integration tests pass).
- Run `examples/demo.rs` to verify end-to-end functionality.
