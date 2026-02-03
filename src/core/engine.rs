use crate::core::log_record::LogRecord;
use crate::core::memtable::MemTable;
use crate::infra::codec::decode;
use crate::infra::config::LsmConfig;
use crate::infra::error::{LsmError, Result};
use crate::storage::sstable::SStable;
use crate::storage::wal::WriteAheadLog;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tracing::{info, warn};

#[derive(Serialize)]
pub struct LsmStats {
    pub mem_records: usize,
    pub mem_kb: usize,
    pub sst_files: usize,
    pub sst_records: u64,
    pub sst_kb: u64,
    pub wal_kb: u64,
    pub total_records: u64,
    pub memtable_max_size: usize,
}

pub struct LsmEngine {
    pub(crate) memtable: Mutex<MemTable>,
    pub(crate) wal: WriteAheadLog,
    pub(crate) sstables: Mutex<Vec<SStable>>,
    pub(crate) dir_path: PathBuf,
    pub(crate) config: LsmConfig,
}

impl LsmEngine {
    pub fn new(config: LsmConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.core.dir_path)?;

        let wal = WriteAheadLog::new(&config.core.dir_path)?;
        let wal_records = wal.recover()?;

        let mut sstables = Vec::new();
        for entry in std::fs::read_dir(&config.core.dir_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "sst") {
                match SStable::load(&path) {
                    Ok(sst) => sstables.push(sst),
                    Err(e) => warn!("Failed to load SSTable {}: {}", path.display(), e),
                }
            }
        }

        sstables.sort_by(|a, b| b.metadata.timestamp.cmp(&a.metadata.timestamp));

        let mut memtable = MemTable::new(config.core.memtable_max_size);
        for record in wal_records {
            memtable.insert(record);
        }

        info!(
            "LSM Engine initialized: {} sstables, memtable={} records",
            sstables.len(),
            memtable.data.len()
        );

        Ok(Self {
            memtable: Mutex::new(memtable),
            wal,
            sstables: Mutex::new(sstables),
            dir_path: config.core.dir_path.clone(),
            config,
        })
    }

    fn memtable_lock(&self) -> Result<MutexGuard<'_, MemTable>> {
        self.memtable
            .lock()
            .map_err(|_| LsmError::LockPoisoned("memtable"))
    }

    fn sstables_lock(&self) -> Result<MutexGuard<'_, Vec<SStable>>> {
        self.sstables
            .lock()
            .map_err(|_| LsmError::LockPoisoned("sstables"))
    }

    pub fn set(&self, key: String, value: Vec<u8>) -> Result<()> {
        let record = LogRecord::new(key, value);
        self.wal.write_record(&record)?;

        let mut memtable = self.memtable_lock()?;
        memtable.insert(record);

        if memtable.should_flush() {
            drop(memtable);
            self.flush()?;
        }

        Ok(())
    }

    pub fn delete(&self, key: String) -> Result<()> {
        let record = LogRecord::tombstone(key);
        self.wal.write_record(&record)?;

        let mut memtable = self.memtable_lock()?;
        memtable.insert(record);

        if memtable.should_flush() {
            drop(memtable);
            self.flush()?;
        }

        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let memtable = self.memtable_lock()?;
        if let Some(record) = memtable.get(key) {
            return Ok(if record.is_deleted {
                None
            } else {
                Some(record.value)
            });
        }
        drop(memtable);

        let sstables = self.sstables_lock()?;
        for sst in sstables.iter() {
            if let Some(record) = sst.get(key)? {
                return Ok(if record.is_deleted {
                    None
                } else {
                    Some(record.value)
                });
            }
        }

        Ok(None)
    }

    pub fn set_batch(&self, items: Vec<(String, Vec<u8>)>) -> Result<usize> {
        let mut count = 0;
        for (key, value) in items {
            self.set(key, value)?;
            count += 1;
        }
        Ok(count)
    }

    pub fn delete_batch(&self, keys: Vec<String>) -> Result<usize> {
        let mut count = 0;
        for key in keys {
            self.delete(key)?;
            count += 1;
        }
        Ok(count)
    }

    pub fn search(&self, pattern: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let all_data = self.scan()?;
        Ok(all_data
            .into_iter()
            .filter(|(key, _)| key.contains(pattern))
            .collect())
    }

    pub fn search_prefix(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let all_data = self.scan()?;
        Ok(all_data
            .into_iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .collect())
    }

    fn flush(&self) -> Result<()> {
        let mut memtable = self.memtable_lock()?;
        let records: Vec<(String, LogRecord)> = memtable
            .iter_ordered()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if records.is_empty() {
            return Ok(());
        }

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();

        let sst = SStable::create(&self.dir_path, timestamp, &records)?;

        let mut sstables = self.sstables_lock()?;
        sstables.insert(0, sst);
        let cleared = memtable.clear();

        info!(
            "Memtable flushed: {} records, sstables total={}",
            cleared,
            sstables.len()
        );

        drop(memtable);
        drop(sstables);

        self.wal.clear()?;

        Ok(())
    }

    pub fn scan(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let mut result_map: HashMap<String, (Vec<u8>, u128, bool)> = HashMap::new();

        let memtable = self.memtable_lock()?;
        for (key, record) in memtable.iter_ordered() {
            result_map.insert(
                key.clone(),
                (record.value.clone(), record.timestamp, record.is_deleted),
            );
        }
        drop(memtable);

        let sstables = self.sstables_lock()?;
        for sst in sstables.iter() {
            let records = self.read_all_from_sstable(sst)?;
            for record in records {
                result_map.entry(record.key.clone()).or_insert((
                    record.value,
                    record.timestamp,
                    record.is_deleted,
                ));
            }
        }
        drop(sstables);

        let mut results: Vec<(String, Vec<u8>)> = result_map
            .into_iter()
            .filter_map(|(key, (value, _ts, is_deleted))| {
                if !is_deleted {
                    Some((key, value))
                } else {
                    None
                }
            })
            .collect();

        results.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(results)
    }

    fn read_all_from_sstable(&self, sst: &SStable) -> Result<Vec<LogRecord>> {
        use std::fs::File;
        use std::io::{BufReader, Read, Seek, SeekFrom};

        let mut file = BufReader::new(File::open(&sst.path)?);
        file.seek(SeekFrom::Current(8))?;

        let mut len_buf = [0u8; 4];

        file.read_exact(&mut len_buf)?;
        let bloom_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(bloom_len as i64))?;

        file.read_exact(&mut len_buf)?;
        let meta_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(meta_len as i64))?;

        let mut records = Vec::new();
        for _ in 0..sst.metadata.record_count {
            file.read_exact(&mut len_buf)?;
            let record_len = u32::from_le_bytes(len_buf) as usize;
            let mut record_data = vec![0u8; record_len];
            file.read_exact(&mut record_data)?;
            let record: LogRecord = decode(&record_data)?;
            records.push(record);
        }
        Ok(records)
    }

    pub fn keys(&self) -> Result<Vec<String>> {
        let all_data = self.scan()?;
        Ok(all_data.into_iter().map(|(k, _)| k).collect())
    }

    pub fn count(&self) -> Result<usize> {
        Ok(self.scan()?.len())
    }

    pub fn stats(&self) -> String {
        let memtable = match self.memtable_lock() {
            Ok(g) => g,
            Err(e) => return format!("LSM Stats error: {e}"),
        };
        let sstables = match self.sstables_lock() {
            Ok(g) => g,
            Err(e) => return format!("LSM Stats error: {e}"),
        };

        format!(
            "LSM Stats:\n MemTable: {} records, ~{} KB\n SSTables: {} files",
            memtable.data.len(),
            memtable.size_bytes / 1024,
            sstables.len()
        )
    }

    pub fn stats_all(&self) -> std::result::Result<LsmStats, String> {
        let memtable = self.memtable_lock().map_err(|e| e.to_string())?;
        let sstables = self.sstables_lock().map_err(|e| e.to_string())?;

        let mem_records = memtable.data.len();
        let sst_records_total: u64 = sstables
            .iter()
            .map(|s| s.metadata.record_count as u64)
            .sum();

        let sst_bytes_total: u64 = sstables
            .iter()
            .map(|s| std::fs::metadata(&s.path).map(|m| m.len()).unwrap_or(0))
            .sum();

        let wal_bytes: u64 = std::fs::metadata(&self.wal.path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(LsmStats {
            mem_records,
            mem_kb: memtable.size_bytes / 1024,
            sst_files: sstables.len(),
            sst_records: sst_records_total,
            sst_kb: sst_bytes_total / 1024,
            wal_kb: wal_bytes / 1024,
            total_records: (mem_records as u64) + sst_records_total,
            memtable_max_size: self.config.core.memtable_max_size / 1024,
        })
    }
}
