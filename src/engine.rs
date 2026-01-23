use crate::error::Result;
use crate::log_record::LogRecord;
use crate::memtable::MemTable;
use crate::sstable::SStable;
use crate::wal::WriteAheadLog;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

pub struct LsmConfig {
    pub memtable_max_size: usize,
    pub data_dir: PathBuf,
}

impl Default for LsmConfig {
    fn default() -> Self {
        Self {
            memtable_max_size: 4 * 1024 * 1024,
            data_dir: PathBuf::from("./.lsm_data"),
        }
    }
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
        std::fs::create_dir_all(&config.data_dir)?;

        let wal = WriteAheadLog::new(&config.data_dir)?;
        let wal_records = wal.recover()?;

        let mut sstables = Vec::new();
        for entry in std::fs::read_dir(&config.data_dir)? {
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

        let mut memtable = MemTable::new(config.memtable_max_size);
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
            dir_path: config.data_dir.clone(),
            config,
        })
    }

    pub fn set(&self, key: String, value: Vec<u8>) -> Result<()> {
        let record = LogRecord::new(key, value);
        self.wal.write_record(&record)?;

        let mut memtable = self.memtable.lock().unwrap();
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

        let mut memtable = self.memtable.lock().unwrap();
        memtable.insert(record);
        if memtable.should_flush() {
            drop(memtable);
            self.flush()?;
        }
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let memtable = self.memtable.lock().unwrap();
        if let Some(record) = memtable.get(key) {
            return Ok(if record.is_deleted {
                None
            } else {
                Some(record.value)
            });
        }
        drop(memtable);

        let sstables = self.sstables.lock().unwrap();
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

    fn flush(&self) -> Result<()> {
        let mut memtable = self.memtable.lock().unwrap();
        let records: Vec<(String, LogRecord)> = memtable
            .iter_ordered()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if records.is_empty() {
            return Ok(());
        }

        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let sst = SStable::create(&self.dir_path, timestamp, &records)?;
        let mut sstables = self.sstables.lock().unwrap();
        sstables.insert(0, sst);

        let cleared = memtable.clear();
        info!(
            "Memtable flushed: {} records, sstables={}",
            cleared,
            sstables.len()
        );

        drop(memtable);
        drop(sstables);
        self.wal.clear()?;

        // TODO: compaction
        Ok(())
    }

    pub fn stats(&self) -> String {
        let memtable = self.memtable.lock().unwrap();
        let sstables = self.sstables.lock().unwrap();
        format!(
            "LSM Stats:\n  MemTable: {} records, ~{} KB\n  SSTables: {} files",
            memtable.data.len(),
            memtable.size_bytes / 1024,
            sstables.len()
        )
    }
}
