use crate::core::log_record::LogRecord;
use crate::infra::codec::decode;
use crate::infra::config::StorageConfig;
use crate::infra::error::{LsmError, Result};
use crate::storage::block::Block;
use crate::storage::builder::{BlockMeta, MetaBlock};
use bloomfilter::Bloom;
use lru::LruCache;
use lz4_flex::decompress_size_prepended;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::num::NonZeroUsize;
use std::path::PathBuf;

const SST_MAGIC_V2: &[u8; 8] = b"LSMSST02";
const FOOTER_SIZE: u64 = 8;

/// SSTable V2 Reader with sparse index, Bloom filter, and block caching
#[derive(Debug)]
pub struct SstableReader {
    metadata: MetaBlock,
    bloom_filter: Bloom<[u8]>,
    file: File,
    block_cache: LruCache<u64, Vec<u8>>,
    path: PathBuf,
    #[allow(dead_code)]
    config: StorageConfig,
}

impl SstableReader {
    /// Open an SSTable V2 file for reading
    pub fn open(path: PathBuf, config: StorageConfig) -> Result<Self> {
        let mut file = File::open(&path)?;

        // Verify magic number
        let mut magic = [0u8; 8];
        file.read_exact(&mut magic)?;
        if &magic != SST_MAGIC_V2 {
            return Err(LsmError::InvalidSstableFormat(format!(
                "Invalid magic number: expected {:?}, found {:?}",
                SST_MAGIC_V2, magic
            )));
        }

        // Read footer to get metadata offset
        let meta_offset = Self::read_footer(&mut file)?;

        // Read and decompress metadata block
        let metadata = Self::read_meta_block(&mut file, meta_offset)?;

        // Reconstruct Bloom filter from stored data
        let bloom_filter = Self::reconstruct_bloom_filter(&metadata.bloom_filter_data)?;

        // Initialize LRU cache
        let cache_capacity = Self::calculate_cache_capacity(&config);
        let block_cache = LruCache::new(cache_capacity);

        Ok(Self {
            metadata,
            bloom_filter,
            file,
            block_cache,
            path,
            config,
        })
    }

    /// Check if key might exist using Bloom filter (fast pre-check)
    pub fn might_contain(&self, key: &str) -> bool {
        self.bloom_filter.check(key.as_bytes())
    }

    /// Retrieve a value by key using sparse index and Bloom filter
    pub fn get(&mut self, key: &str) -> Result<Option<LogRecord>> {
        // Fast rejection using Bloom filter
        if !self.might_contain(key) {
            return Ok(None);
        }

        // Binary search on sparse index to find the block
        let block_meta = match self.binary_search_block(key.as_bytes()) {
            Some(meta) => meta.clone(),
            None => return Ok(None),
        };

        // Read and decompress the block (with caching)
        let block_data = self.read_block(&block_meta)?;

        // Deserialize block
        let block = Block::decode(&block_data);

        // Linear scan within the block to find the key
        self.search_in_block(&block, key.as_bytes())
    }

    /// Search for a key within a decoded block
    fn search_in_block(&self, block: &Block, key: &[u8]) -> Result<Option<LogRecord>> {
        // Access block data through pub(crate) fields
        for &offset in &block.offsets {
            let offset = offset as usize;
            if offset + 2 > block.data.len() {
                break;
            }

            // Read key length
            let key_len = u16::from_le_bytes([block.data[offset], block.data[offset + 1]]) as usize;
            if offset + 2 + key_len + 2 > block.data.len() {
                break;
            }

            // Read key
            let entry_key = &block.data[offset + 2..offset + 2 + key_len];

            if entry_key == key {
                // Read value length
                let val_len_offset = offset + 2 + key_len;
                let val_len = u16::from_le_bytes([
                    block.data[val_len_offset],
                    block.data[val_len_offset + 1],
                ]) as usize;

                if val_len_offset + 2 + val_len > block.data.len() {
                    break;
                }

                // Read value
                let entry_value = &block.data[val_len_offset + 2..val_len_offset + 2 + val_len];

                // Decode the LogRecord from value
                let record: LogRecord = decode(entry_value)?;
                return Ok(Some(record));
            }
        }

        Ok(None)
    }

    /// Scan all records in the SSTable (for compaction)
    pub fn scan(&mut self) -> Result<Vec<(Vec<u8>, LogRecord)>> {
        let mut records = Vec::new();

        for block_meta in &self.metadata.blocks.clone() {
            let block_data = self.read_block(block_meta)?;
            let block = Block::decode(&block_data);

            // Access block data through pub(crate) fields
            for &offset in &block.offsets {
                let offset = offset as usize;
                if offset + 2 > block.data.len() {
                    break;
                }

                // Read key length
                let key_len =
                    u16::from_le_bytes([block.data[offset], block.data[offset + 1]]) as usize;
                if offset + 2 + key_len + 2 > block.data.len() {
                    break;
                }

                // Read key
                let key = block.data[offset + 2..offset + 2 + key_len].to_vec();

                // Read value length
                let val_len_offset = offset + 2 + key_len;
                let val_len = u16::from_le_bytes([
                    block.data[val_len_offset],
                    block.data[val_len_offset + 1],
                ]) as usize;

                if val_len_offset + 2 + val_len > block.data.len() {
                    break;
                }

                // Read value
                let value = &block.data[val_len_offset + 2..val_len_offset + 2 + val_len];

                // Decode the LogRecord from value
                let record: LogRecord = decode(value)?;
                records.push((key, record));
            }
        }

        Ok(records)
    }

    /// Get metadata information
    pub fn metadata(&self) -> &MetaBlock {
        &self.metadata
    }

    /// Get file path
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    // Private helper methods

    fn read_footer(file: &mut File) -> Result<u64> {
        // Seek to the last 8 bytes (footer)
        file.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;

        let mut footer_bytes = [0u8; 8];
        file.read_exact(&mut footer_bytes)?;

        let meta_offset = u64::from_le_bytes(footer_bytes);
        Ok(meta_offset)
    }

    fn read_meta_block(file: &mut File, offset: u64) -> Result<MetaBlock> {
        // Seek to metadata block
        file.seek(SeekFrom::Start(offset))?;

        // Read compressed metadata until footer
        let file_len = file.metadata()?.len();
        let meta_size = (file_len - offset - FOOTER_SIZE) as usize;

        let mut compressed_meta = vec![0u8; meta_size];
        file.read_exact(&mut compressed_meta)?;

        // Decompress metadata
        let decompressed = decompress_size_prepended(&compressed_meta).map_err(|e| {
            LsmError::DecompressionFailed(format!("Metadata decompression failed: {}", e))
        })?;

        // Deserialize metadata
        let metadata: MetaBlock = decode(&decompressed)?;
        Ok(metadata)
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

    fn read_and_decompress_block(&mut self, block_meta: &BlockMeta) -> Result<Vec<u8>> {
        // Seek to block offset
        self.file.seek(SeekFrom::Start(block_meta.offset))?;

        // Read compressed block
        let mut compressed_block = vec![0u8; block_meta.size as usize];
        self.file.read_exact(&mut compressed_block)?;

        // Decompress block
        let decompressed = decompress_size_prepended(&compressed_block).map_err(|e| {
            LsmError::DecompressionFailed(format!(
                "Block decompression failed at offset {}: {}",
                block_meta.offset, e
            ))
        })?;

        // Verify decompressed size matches metadata
        if decompressed.len() != block_meta.uncompressed_size as usize {
            return Err(LsmError::CorruptedData(format!(
                "Block size mismatch: expected {}, got {}",
                block_meta.uncompressed_size,
                decompressed.len()
            )));
        }

        Ok(decompressed)
    }

    fn binary_search_block(&self, key: &[u8]) -> Option<&BlockMeta> {
        // If key is smaller than the first key in the SSTable, it doesn't exist
        if key < self.metadata.min_key.as_slice() {
            return None;
        }

        // If key is larger than the last key in the SSTable, it doesn't exist
        if key > self.metadata.max_key.as_slice() {
            return None;
        }

        // Binary search using partition_point to find the block where first_key <= search_key
        let idx = self
            .metadata
            .blocks
            .partition_point(|block_meta| block_meta.first_key.as_slice() <= key);

        // If idx is 0, key is smaller than all first_keys
        if idx == 0 {
            return None;
        }

        // Return the block at idx - 1 (the last block where first_key <= search_key)
        Some(&self.metadata.blocks[idx - 1])
    }

    fn calculate_cache_capacity(config: &StorageConfig) -> NonZeroUsize {
        let cache_size_bytes = config.block_cache_size_mb * 1024 * 1024;
        let avg_block_size = config.block_size;
        let capacity = (cache_size_bytes / avg_block_size).max(1);
        NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap())
    }

    fn reconstruct_bloom_filter(data: &[u8]) -> Result<Bloom<[u8]>> {
        // The bloom filter data should contain: [bitmap_size (8 bytes)][items_count (8 bytes)][seed (32 bytes)][bitmap data...]
        if data.len() < 48 {
            return Err(LsmError::CorruptedData(
                "Invalid bloom filter data".to_string(),
            ));
        }

        let bitmap_size = u64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]) as usize;

        let items_count = u64::from_le_bytes([
            data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
        ]) as usize;

        let mut seed = [0u8; 32];
        seed.copy_from_slice(&data[16..48]);

        let bloom = Bloom::<[u8]>::new_with_seed(bitmap_size, items_count, &seed).map_err(|e| {
            LsmError::CompactionFailed(format!("Bloom filter reconstruction failed: {}", e))
        })?;

        Ok(bloom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::builder::SstableBuilder;
    use tempfile::tempdir;

    fn create_test_record(key: &str, value: &[u8]) -> LogRecord {
        LogRecord::new(key.to_string(), value.to_vec())
    }

    #[test]
    fn test_reader_basic_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.sst");
        let config = StorageConfig::default();

        // Write SSTable
        let mut builder = SstableBuilder::new(path.clone(), config.clone(), 123).unwrap();
        builder
            .add(b"key1", &create_test_record("key1", b"value1"))
            .unwrap();
        builder
            .add(b"key2", &create_test_record("key2", b"value2"))
            .unwrap();
        builder
            .add(b"key3", &create_test_record("key3", b"value3"))
            .unwrap();
        builder.finish().unwrap();

        // Read SSTable
        let mut reader = SstableReader::open(path, config).unwrap();

        // Verify reads
        let record1 = reader.get("key1").unwrap().unwrap();
        assert_eq!(record1.value, b"value1");

        let record2 = reader.get("key2").unwrap().unwrap();
        assert_eq!(record2.value, b"value2");

        let record3 = reader.get("key3").unwrap().unwrap();
        assert_eq!(record3.value, b"value3");

        // Verify non-existent key
        assert!(reader.get("key4").unwrap().is_none());
    }

    #[test]
    fn test_reader_bloom_filter() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bloom_test.sst");
        let config = StorageConfig::default();

        // Write SSTable with known keys
        let mut builder = SstableBuilder::new(path.clone(), config.clone(), 456).unwrap();
        for i in 0..100 {
            let key = format!("key_{:03}", i);
            builder
                .add(key.as_bytes(), &create_test_record(&key, b"value"))
                .unwrap();
        }
        builder.finish().unwrap();

        // Read and test Bloom filter
        let reader = SstableReader::open(path, config).unwrap();

        // Keys that exist should pass Bloom filter
        assert!(reader.might_contain("key_000"));
        assert!(reader.might_contain("key_050"));
        assert!(reader.might_contain("key_099"));

        // Non-existent keys might have false positives, but should mostly return false
        let false_positive_count = (1000..1100)
            .filter(|i| reader.might_contain(&format!("nonexistent_{}", i)))
            .count();

        // With 1% FP rate and 100 checks, expect < 5 false positives
        assert!(
            false_positive_count < 5,
            "Too many false positives: {}",
            false_positive_count
        );
    }

    #[test]
    fn test_reader_multiple_blocks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("multi_block.sst");
        let mut config = StorageConfig::default();
        config.block_size = 256; // Small blocks to force multiple blocks

        // Write many records to span multiple blocks
        let mut builder = SstableBuilder::new(path.clone(), config.clone(), 789).unwrap();
        for i in 0..50 {
            let key = format!("key_{:03}", i);
            let value = vec![b'x'; 20];
            builder
                .add(key.as_bytes(), &create_test_record(&key, &value))
                .unwrap();
        }
        builder.finish().unwrap();

        // Read and verify all records
        let mut reader = SstableReader::open(path, config).unwrap();
        for i in 0..50 {
            let key = format!("key_{:03}", i);
            let record = reader.get(&key).unwrap();
            assert!(record.is_some(), "Key {} should exist", key);
        }
    }

    #[test]
    fn test_reader_boundary_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("boundary.sst");
        let config = StorageConfig::default();

        // Write records with boundary keys
        let mut builder = SstableBuilder::new(path.clone(), config.clone(), 111).unwrap();
        builder
            .add(b"aaa", &create_test_record("aaa", b"first"))
            .unwrap();
        builder
            .add(b"mmm", &create_test_record("mmm", b"middle"))
            .unwrap();
        builder
            .add(b"zzz", &create_test_record("zzz", b"last"))
            .unwrap();
        builder.finish().unwrap();

        let mut reader = SstableReader::open(path, config).unwrap();

        // Test exact boundary keys
        assert!(
            reader.get("aaa").unwrap().is_some(),
            "First key should exist"
        );
        assert!(
            reader.get("zzz").unwrap().is_some(),
            "Last key should exist"
        );

        // Test keys before first
        assert!(
            reader.get("000").unwrap().is_none(),
            "Key before first should not exist"
        );
        assert!(
            reader.get("aa").unwrap().is_none(),
            "Key before first should not exist"
        );

        // Test keys after last
        assert!(
            reader.get("zzzz").unwrap().is_none(),
            "Key after last should not exist"
        );

        // Test keys between boundaries
        assert!(
            reader.get("bbb").unwrap().is_none(),
            "Non-existent key should not exist"
        );
        assert!(
            reader.get("mmm").unwrap().is_some(),
            "Middle key should exist"
        );
    }

    #[test]
    fn test_reader_scan() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scan_test.sst");
        let config = StorageConfig::default();

        // Write ordered records
        let mut builder = SstableBuilder::new(path.clone(), config.clone(), 999).unwrap();
        let test_keys = vec!["apple", "banana", "cherry"];

        for key in &test_keys {
            builder
                .add(
                    key.as_bytes(),
                    &create_test_record(key, format!("{}_value", key).as_bytes()),
                )
                .unwrap();
        }
        builder.finish().unwrap();

        // Scan all records
        let mut reader = SstableReader::open(path, config).unwrap();
        let records = reader.scan().unwrap();

        assert_eq!(records.len(), test_keys.len(), "Should scan all records");
    }

    #[test]
    fn test_reader_invalid_magic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("invalid.sst");

        // Write file with wrong magic number
        std::fs::write(&path, b"INVALID_MAGIC").unwrap();

        let config = StorageConfig::default();
        let result = SstableReader::open(path, config);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            LsmError::InvalidSstableFormat(_)
        ));
    }
}
