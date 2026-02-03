use crate::core::log_record::LogRecord;
use crate::infra::codec::{decode, encode};
use crate::infra::config::StorageConfig;
use crate::infra::error::{LsmError, Result};

use bloomfilter::Bloom;
use crc32fast;
use serde::{Deserialize, Serialize};

use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use tracing::debug;

const SST_MAGIC: &[u8; 8] = b"LSMSST01";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SstableMetadata {
    pub timestamp: u128,
    pub min_key: String,
    pub max_key: String,
    pub record_count: u32,
    pub checksum: u32,
}

/// Metadata for a single block in the sparse index
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockMeta {
    pub first_key: String,
    pub offset: u64,
    pub size: u32,
}

#[derive(Debug)]
pub struct SStable {
    pub(crate) metadata: SstableMetadata,
    pub(crate) bloom_filter: Bloom<[u8]>,
    pub(crate) index: Vec<BlockMeta>,
    pub(crate) file: File,
    pub(crate) path: PathBuf,
}

fn read_and_check_magic<R: Read>(mut r: R) -> Result<()> {
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != SST_MAGIC {
        return Err(LsmError::InvalidSstable);
    }
    Ok(())
}

impl SStable {
    pub fn create(
        dir_path: &Path,
        timestamp: u128,
        config: &StorageConfig,
        records: &[(String, LogRecord)],
    ) -> Result<Self> {
        if records.is_empty() {
            return Err(LsmError::CompactionFailed(
                "Cannot create SSTable with empty records".to_string(),
            ));
        }

        let path = dir_path.join(format!("{}.sst", timestamp));
        let mut file = BufWriter::new(File::create(&path)?);

        // 1) Header: Magic bytes
        file.write_all(SST_MAGIC)?;

        // 2) Bloom Filter
        let mut bloom =
            Bloom::<[u8]>::new_for_fp_rate(records.len(), config.bloom_false_positive_rate)
                .map_err(|e| LsmError::CompactionFailed(e.to_string()))?;

        for (key, _) in records.iter() {
            bloom.set(key.as_bytes());
        }

        let bloom_bytes = bloom.into_bytes();
        file.write_all(&(bloom_bytes.len() as u32).to_le_bytes())?;
        file.write_all(&bloom_bytes)?;

        // 3) Metadata
        let checksum = crc32fast::hash(&encode(&records)?); // Checksum over serialized records

        let metadata = SstableMetadata {
            timestamp,
            min_key: records[0].0.clone(),
            max_key: records[records.len() - 1].0.clone(),
            record_count: records.len() as u32,
            checksum,
        };

        let metadata_bytes = encode(&metadata)?;
        file.write_all(&(metadata_bytes.len() as u32).to_le_bytes())?;
        file.write_all(&metadata_bytes)?;

        // 4) Write blocks and build sparse index
        let mut index = Vec::new();
        let mut current_block = Vec::new();
        let mut current_block_size = 0usize;
        let blocks_start_offset = file.stream_position()?;
        let mut current_offset = blocks_start_offset;
        let block_size = config.block_size;

        for (key, record) in records.iter() {
            let record_bytes = encode(record)?;
            let entry_size = 4 + record_bytes.len(); // u32 length + data

            // Check if adding this record would exceed block size
            if current_block_size + entry_size > block_size && !current_block.is_empty() {
                // Write current block
                let block_data = serialize_block(&current_block)?;
                file.write_all(&block_data)?;

                // Add to index
                index.push(BlockMeta {
                    first_key: current_block[0].0.clone(),
                    offset: current_offset,
                    size: block_data.len() as u32,
                });

                current_offset += block_data.len() as u64;
                current_block.clear();
                current_block_size = 0;
            }

            current_block.push((key.clone(), record.clone()));
            current_block_size += entry_size;
        }

        // Write last block
        if !current_block.is_empty() {
            let block_data = serialize_block(&current_block)?;
            file.write_all(&block_data)?;

            index.push(BlockMeta {
                first_key: current_block[0].0.clone(),
                offset: current_offset,
                size: block_data.len() as u32,
            });
        }

        // 5) Write sparse index
        let index_offset = file.stream_position()?;
        let index_bytes = encode(&index)?;
        file.write_all(&(index_bytes.len() as u32).to_le_bytes())?;
        file.write_all(&index_bytes)?;

        // 6) Write footer (index offset)
        file.write_all(&index_offset.to_le_bytes())?;

        file.flush()?;
        file.get_ref().sync_all()?;

        debug!(
            "SSTable created: {}, records={}, blocks={}, checksum={}",
            path.display(),
            metadata.record_count,
            index.len(),
            metadata.checksum
        );

        // 7) Open file for reading and rebuild bloom from bytes
        let read_file = File::open(&path)?;
        let bloom_filter = Bloom::<[u8]>::from_bytes(bloom_bytes)
            .map_err(|e| LsmError::CompactionFailed(e.to_string()))?;

        Ok(Self {
            metadata,
            bloom_filter,
            index,
            file: read_file,
            path,
        })
    }

    /// Open an existing SSTable file with lazy loading (only footer + index)
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;

        // 1. Read footer (last 8 bytes = index offset)
        file.seek(SeekFrom::End(-8))?;
        let mut footer = [0u8; 8];
        file.read_exact(&mut footer)?;
        let index_offset = u64::from_le_bytes(footer);

        // 2. Read sparse index
        file.seek(SeekFrom::Start(index_offset))?;
        let mut len_buf = [0u8; 4];
        file.read_exact(&mut len_buf)?;
        let index_len = u32::from_le_bytes(len_buf) as usize;
        let mut index_data = vec![0u8; index_len];
        file.read_exact(&mut index_data)?;
        let index: Vec<BlockMeta> = decode(&index_data)?;

        if index.is_empty() {
            return Err(LsmError::InvalidSstable);
        }

        // 3. Read header, bloom filter, and metadata
        file.seek(SeekFrom::Start(0))?;
        read_and_check_magic(&mut file)?;

        // Bloom filter
        file.read_exact(&mut len_buf)?;
        let bloom_len = u32::from_le_bytes(len_buf) as usize;
        let mut bloom_data = vec![0u8; bloom_len];
        file.read_exact(&mut bloom_data)?;
        let bloom = Bloom::<[u8]>::from_bytes(bloom_data)
            .map_err(|_| LsmError::InvalidSstable)?;

        // Metadata
        file.read_exact(&mut len_buf)?;
        let meta_len = u32::from_le_bytes(len_buf) as usize;
        let mut meta_data = vec![0u8; meta_len];
        file.read_exact(&mut meta_data)?;
        let metadata: SstableMetadata = decode(&meta_data)?;

        debug!(
            "SSTable opened: {}, records={}, blocks={}",
            path.display(),
            metadata.record_count,
            index.len()
        );

        Ok(Self {
            metadata,
            bloom_filter: bloom,
            index,
            file,
            path: path.to_path_buf(),
        })
    }

    /// Legacy load method for backward compatibility (delegates to open)
    pub fn load(path: &Path) -> Result<Self> {
        Self::open(path)
    }

    /// Read a specific block from disk
    fn read_block(&mut self, block_meta: &BlockMeta) -> Result<Vec<LogRecord>> {
        self.file.seek(SeekFrom::Start(block_meta.offset))?;

        let mut block_data = vec![0u8; block_meta.size as usize];
        self.file.read_exact(&mut block_data)?;

        deserialize_block(&block_data)
    }

    pub fn get(&mut self, key: &str) -> Result<Option<LogRecord>> {
        // 1. Check bloom filter
        if !self.bloom_filter.check(key.as_bytes()) {
            return Ok(None);
        }

        // 2. Binary search on sparse index using partition_point
        // Find the first block where first_key > search_key
        let block_idx = self.index.partition_point(|block_meta| {
            block_meta.first_key.as_str() <= key
        });

        // Edge case: key is smaller than the first key of the first block
        if block_idx == 0 {
            return Ok(None);
        }

        // The candidate block is at index block_idx - 1
        let candidate_idx = block_idx - 1;
        let block_meta = &self.index[candidate_idx].clone();

        // 3. Load the block from disk
        let records = self.read_block(block_meta)?;

        // 4. Linear search within the block
        for record in records {
            if record.key == key {
                return Ok(Some(record));
            }
        }

        Ok(None)
    }
}

/// Serialize a block of records into bytes
fn serialize_block(records: &[(String, LogRecord)]) -> Result<Vec<u8>> {
    let mut block_data = Vec::new();
    for (_key, record) in records {
        let record_bytes = encode(record)?;
        let len = record_bytes.len() as u32;
        block_data.extend_from_slice(&len.to_le_bytes());
        block_data.extend_from_slice(&record_bytes);
    }
    Ok(block_data)
}

/// Deserialize a block of bytes into records
fn deserialize_block(block_data: &[u8]) -> Result<Vec<LogRecord>> {
    let mut cursor = std::io::Cursor::new(block_data);
    let mut records = Vec::new();
    let mut len_buf = [0u8; 4];

    while cursor.position() < block_data.len() as u64 {
        cursor.read_exact(&mut len_buf)?;
        let record_len = u32::from_le_bytes(len_buf) as usize;

        let mut record_data = vec![0u8; record_len];
        cursor.read_exact(&mut record_data)?;

        let record: LogRecord = decode(&record_data)?;
        records.push(record);
    }

    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sstable_create_and_open() {
        let dir = tempdir().unwrap();
        let timestamp = 12345u128;
        let config = StorageConfig::default();

        // Create test records
        let records: Vec<(String, LogRecord)> = (0..100)
            .map(|i| {
                let key = format!("key_{:03}", i);
                let record = LogRecord::new(key.clone(), format!("value_{}", i).into_bytes());
                (key, record)
            })
            .collect();

        // Create SSTable
        let sstable = SStable::create(dir.path(), timestamp, &config, &records).unwrap();
        assert_eq!(sstable.metadata.record_count, 100);
        assert!(sstable.index.len() > 0);

        // Close and reopen
        drop(sstable);
        let mut reopened = SStable::open(&dir.path().join(format!("{}.sst", timestamp))).unwrap();
        assert_eq!(reopened.metadata.record_count, 100);
        assert!(reopened.index.len() > 0);

        // Test get operations
        let result = reopened.get("key_050").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().key, "key_050");

        // Test non-existent key
        let result = reopened.get("key_999").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_sparse_index_edge_cases() {
        let dir = tempdir().unwrap();
        let timestamp = 67890u128;
        let config = StorageConfig::default();

        let records: Vec<(String, LogRecord)> = vec![
            ("apple".to_string(), LogRecord::new("apple".to_string(), b"a".to_vec())),
            ("banana".to_string(), LogRecord::new("banana".to_string(), b"b".to_vec())),
            ("cherry".to_string(), LogRecord::new("cherry".to_string(), b"c".to_vec())),
        ];

        let mut sstable = SStable::create(dir.path(), timestamp, &config, &records).unwrap();

        // Key before first key
        assert!(sstable.get("aardvark").unwrap().is_none());

        // Exact first key
        assert!(sstable.get("apple").unwrap().is_some());

        // Key after last key
        assert!(sstable.get("zebra").unwrap().is_none());

        // Middle key
        assert!(sstable.get("banana").unwrap().is_some());
    }
}
