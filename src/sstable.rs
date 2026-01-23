use crate::error::{LsmError, Result};
use crate::log_record::LogRecord;
use bincode::{deserialize, serialize};
use bloomfilter::Bloom;
use crc32fast;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tracing::debug;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SstableMetadata {
    pub timestamp: u128,
    pub min_key: String,
    pub max_key: String,
    pub record_count: usize,
    pub checksum: u32,
}

pub struct SStable {
    pub(crate) metadata: SstableMetadata,
    pub(crate) bloom_filter: Bloom<[u8]>,
    pub(crate) path: PathBuf,
}

impl SStable {
    pub fn create(
        dir_path: &Path,
        timestamp: u128,
        records: &[(String, LogRecord)],
    ) -> Result<Self> {
        if records.is_empty() {
            return Err(LsmError::CompactionFailed(
                "Cannot create SSTable with empty records".to_string(),
            ));
        }

        let path = dir_path.join(format!("{}.sst", timestamp));
        let mut file = BufWriter::new(File::create(&path)?);

        // 1. Criar Bloom Filter
        let mut bloom = Bloom::<[u8]>::new_for_fp_rate(records.len(), 0.01)
            .map_err(|e| LsmError::CompactionFailed(e.to_string()))?;

        for (key, _) in records.iter() {
            bloom.set(key.as_bytes());
        }

        // Converte bloom em bytes (consome o bloom original)
        let bloom_bytes = bloom.into_bytes();

        // 2. Serializar records
        let mut records_blob = Vec::new();
        for (_key, record) in records.iter() {
            let record_bytes = serialize(record)?;
            let len = record_bytes.len() as u32;
            records_blob.extend_from_slice(&len.to_le_bytes());
            records_blob.extend_from_slice(&record_bytes);
        }
        let checksum = crc32fast::hash(&records_blob);

        // 3. Metadados
        let metadata = SstableMetadata {
            timestamp,
            min_key: records[0].0.clone(),
            max_key: records[records.len() - 1].0.clone(),
            record_count: records.len(),
            checksum,
        };
        let metadata_bytes = serialize(&metadata)?;

        // 4. Escrever no arquivo
        file.write_all(&(bloom_bytes.len() as u32).to_le_bytes())?;
        file.write_all(&bloom_bytes)?;
        file.write_all(&(metadata_bytes.len() as u32).to_le_bytes())?;
        file.write_all(&metadata_bytes)?;
        file.write_all(&records_blob)?;
        file.flush()?;
        file.get_ref().sync_all()?;

        debug!(
            "SSTable created: {}, records={}, checksum={}",
            path.display(),
            metadata.record_count,
            metadata.checksum
        );

        // 5. Reconstruir bloom a partir dos bytes salvos
        let bloom_filter = Bloom::<[u8]>::from_bytes(bloom_bytes)
            .map_err(|e| LsmError::CompactionFailed(e.to_string()))?;

        Ok(Self {
            metadata,
            bloom_filter,
            path,
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let mut file = BufReader::new(File::open(path)?);
        let mut len_buf = [0u8; 4];

        file.read_exact(&mut len_buf)?;
        let bloom_len = u32::from_le_bytes(len_buf) as usize;
        let mut bloom_data = vec![0u8; bloom_len];
        file.read_exact(&mut bloom_data)?;
        let bloom = Bloom::<[u8]>::from_bytes(bloom_data).map_err(|_| LsmError::InvalidSstable)?;

        file.read_exact(&mut len_buf)?;
        let meta_len = u32::from_le_bytes(len_buf) as usize;
        let mut meta_data = vec![0u8; meta_len];
        file.read_exact(&mut meta_data)?;
        let metadata: SstableMetadata = deserialize(&meta_data)?;

        Ok(Self {
            metadata,
            bloom_filter: bloom,
            path: path.to_path_buf(),
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<LogRecord>> {
        if !self.bloom_filter.check(key.as_bytes()) {
            return Ok(None);
        }

        let mut file = BufReader::new(File::open(&self.path)?);
        let mut len_buf = [0u8; 4];

        file.read_exact(&mut len_buf)?;
        let bloom_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(bloom_len as i64))?;

        file.read_exact(&mut len_buf)?;
        let meta_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(meta_len as i64))?;

        for _ in 0..self.metadata.record_count {
            file.read_exact(&mut len_buf)?;
            let record_len = u32::from_le_bytes(len_buf) as usize;

            let mut record_data = vec![0u8; record_len];
            file.read_exact(&mut record_data)?;
            let record: LogRecord = deserialize(&record_data)?;
            if record.key == key {
                return Ok(Some(record));
            }
        }

        Ok(None)
    }
}
