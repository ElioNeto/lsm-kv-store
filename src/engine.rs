use crate::error::Result;
use crate::log_record::LogRecord;
use crate::memtable::MemTable;
use crate::sstable::SStable;
use crate::wal::WriteAheadLog;
// sstable.rs usa um header fixo "LSMSST01" (8 bytes)
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

#[derive(Clone, Debug)]
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
}

impl LsmEngine {
    pub fn search(&self, pattern: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let all_data = self.scan()?;

        Ok(all_data
            .into_iter()
            .filter(|(key, _)| key.contains(pattern))
            .collect())
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

    pub fn search_prefix(&self, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let all_data = self.scan()?;

        Ok(all_data
            .into_iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .collect())
    }

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

    pub fn stats_all(&self) -> String {
        let memtable = self.memtable.lock().unwrap();
        let sstables = self.sstables.lock().unwrap();

        let mem_records = memtable.data.len();
        let mem_kb = memtable.size_bytes / 1024;

        let sst_files = sstables.len();
        let sst_records_total: usize = sstables.iter().map(|s| s.metadata.record_count).sum();

        let sst_bytes_total: u64 = sstables
            .iter()
            .map(|s| std::fs::metadata(&s.path).map(|m| m.len()).unwrap_or(0))
            .sum();

        let wal_bytes: u64 = std::fs::metadata(&self.wal.path)
            .map(|m| m.len())
            .unwrap_or(0);

        format!(
            "LSM Stats:\n\
         MemTable: {} records, ~{} KB\n\
         SSTables: {} files, {} records (raw), ~{} KB on disk\n\
         WAL: ~{} KB",
            mem_records,
            mem_kb,
            sst_files,
            sst_records_total,
            (sst_bytes_total / 1024),
            (wal_bytes / 1024),
        )
    }

    pub fn scan(&self) -> Result<Vec<(String, Vec<u8>)>> {
        let mut result_map: HashMap<String, (Vec<u8>, u128, bool)> = HashMap::new();

        // 1. Coletar da MemTable (mais alta prioridade)
        let memtable = self.memtable.lock().unwrap();
        for (key, record) in memtable.iter_ordered() {
            result_map.insert(
                key.clone(),
                (record.value.clone(), record.timestamp, record.is_deleted),
            );
        }
        drop(memtable);

        // 2. Coletar de SSTables (já ordenadas por timestamp decrescente)
        let sstables = self.sstables.lock().unwrap();
        for sst in sstables.iter() {
            // Ler todos os registros da SSTable
            let records = self.read_all_from_sstable(sst)?;

            for record in records {
                // Inserir apenas se a chave ainda não existir (mais recente já foi inserida)
                result_map.entry(record.key.clone()).or_insert((
                    record.value,
                    record.timestamp,
                    record.is_deleted,
                ));
            }
        }
        drop(sstables);

        // 3. Filtrar tombstones e converter para Vec
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

        // 4. Ordenar alfabeticamente por chave
        results.sort_by(|a, b| a.0.cmp(&b.0));

        Ok(results)
    }

    fn read_all_from_sstable(&self, sst: &SStable) -> Result<Vec<LogRecord>> {
        use bincode::deserialize;
        use std::fs::File;
        use std::io::{BufReader, Read, Seek, SeekFrom};

        let mut file = BufReader::new(File::open(&sst.path)?);

        // Pular magic/version (8 bytes)
        file.seek(SeekFrom::Current(8))?;

        let mut len_buf = [0u8; 4];

        // Pular Bloom Filter
        file.read_exact(&mut len_buf)?;
        let bloom_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(bloom_len as i64))?;

        // Pular Metadata
        file.read_exact(&mut len_buf)?;
        let meta_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(meta_len as i64))?;

        // Ler todos os registros
        let mut records = Vec::new();
        for _ in 0..sst.metadata.record_count {
            file.read_exact(&mut len_buf)?;
            let record_len = u32::from_le_bytes(len_buf) as usize;

            let mut record_data = vec![0u8; record_len];
            file.read_exact(&mut record_data)?;

            let record: LogRecord = deserialize(&record_data)?;
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
}
