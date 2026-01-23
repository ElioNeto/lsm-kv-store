//! # LSM-Tree Key-Value Store (Fase 1: Storage Engine)
//!
//! Componentes:
//! - MemTable: BTreeMap (ordem alfabética)
//! - WAL: Write-Ahead Log (append-only + sync_all)
//! - SSTables: arquivos imutáveis com Bloom Filter no cabeçalho
//! - Compaction: estrutura (TODO)

// Crates obrigatórias
use bincode::{deserialize, serialize};
use bloomfilter::Bloom;
use serde::{Deserialize, Serialize};

// Std
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};

use thiserror::Error;
use tracing::{debug, info, warn};

/// Erros possíveis durante operações do LSM-Tree
#[derive(Error, Debug)]
pub enum LsmError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("System time error: {0}")]
    Time(#[from] SystemTimeError),

    #[error("Key not found")]
    KeyNotFound,

    #[error("Invalid SSTable format")]
    InvalidSstable,

    #[error("Compaction failed: {0}")]
    CompactionFailed(String),

    #[error("WAL corruption detected")]
    WalCorruption,
}

pub type Result<T> = std::result::Result<T, LsmError>;

/// ============================================================================
/// PART 1: DATA STRUCTURES
/// ============================================================================

/// Registro de log (LogRecord) serializado em binário
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogRecord {
    pub key: String,
    pub value: Vec<u8>,
    pub timestamp: u128, // nanos
    pub is_deleted: bool,
}

impl LogRecord {
    /// Cria um novo LogRecord com timestamp atual
    pub fn new(key: String, value: Vec<u8>) -> Self {
        Self {
            key,
            value,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            is_deleted: false,
        }
    }

    /// Cria um LogRecord de deleção (tombstone)
    pub fn tombstone(key: String) -> Self {
        Self {
            key,
            value: Vec::new(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            is_deleted: true,
        }
    }
}

/// Metadados persistidos do SSTable (cabeçalho)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SstableMetadata {
    pub timestamp: u128,
    pub min_key: String,
    pub max_key: String,
    pub record_count: usize,

    /// Checksum (CRC32) calculado sobre o “blob” dos registros (length+payload de cada record).
    /// Obs.: verificação na leitura pode ser adicionada depois (TODO).
    pub checksum: u32,
}

/// ============================================================================
/// PART 2: MEMTABLE
/// ============================================================================

struct MemTable {
    data: BTreeMap<String, LogRecord>,
    size_bytes: usize,
    max_size_bytes: usize,
}

impl MemTable {
    fn new(max_size_bytes: usize) -> Self {
        Self {
            data: BTreeMap::new(),
            size_bytes: 0,
            max_size_bytes,
        }
    }

    fn insert(&mut self, record: LogRecord) {
        let record_size = Self::estimate_size(&record);
        if let Some(old_record) = self.data.insert(record.key.clone(), record) {
            self.size_bytes = self
                .size_bytes
                .saturating_sub(Self::estimate_size(&old_record));
        }
        self.size_bytes += record_size;
    }

    fn should_flush(&self) -> bool {
        self.size_bytes >= self.max_size_bytes
    }

    fn get(&self, key: &str) -> Option<LogRecord> {
        self.data.get(key).cloned()
    }

    fn iter_ordered(&self) -> impl Iterator<Item = (&String, &LogRecord)> {
        self.data.iter()
    }

    fn clear(&mut self) -> usize {
        let count = self.data.len();
        self.data.clear();
        self.size_bytes = 0;
        count
    }

    fn estimate_size(record: &LogRecord) -> usize {
        record.key.len() + record.value.len() + 32
    }
}

/// ============================================================================
/// PART 3: WRITE-AHEAD LOG (WAL)
/// ============================================================================

struct WriteAheadLog {
    file: Mutex<BufWriter<File>>,
    path: PathBuf,
}

impl WriteAheadLog {
    fn new(dir_path: &Path) -> Result<Self> {
        let wal_path = dir_path.join("wal.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&wal_path)?;

        Ok(Self {
            file: Mutex::new(BufWriter::new(file)),
            path: wal_path,
        })
    }

    /// Formato: [u32 length][record_bytes]...
    fn write_record(&self, record: &LogRecord) -> Result<()> {
        let serialized = serialize(record)?;
        let length = serialized.len() as u32;

        let mut writer = self.file.lock().unwrap();
        writer.write_all(&length.to_le_bytes())?;
        writer.write_all(&serialized)?;
        writer.flush()?;
        writer.get_ref().sync_all()?; // durabilidade

        debug!("WAL persisted: key={}, ts={}", record.key, record.timestamp);
        Ok(())
    }

    fn recover(&self) -> Result<Vec<LogRecord>> {
        let mut records = Vec::new();
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);

        let mut length_buf = [0u8; 4];
        loop {
            match reader.read_exact(&mut length_buf) {
                Ok(()) => {
                    let length = u32::from_le_bytes(length_buf) as usize;
                    let mut buffer = vec![0u8; length];
                    reader.read_exact(&mut buffer)?;

                    let record: LogRecord =
                        deserialize(&buffer).map_err(|_| LsmError::WalCorruption)?;
                    records.push(record);
                }
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
        }

        info!("WAL recovery: {} records", records.len());
        Ok(records)
    }

    fn clear(&self) -> Result<()> {
        // Trunca o arquivo no disco e reinicia o writer para append.
        let new_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        new_file.sync_all()?;

        let append_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut guard = self.file.lock().unwrap();
        *guard = BufWriter::new(append_file);

        Ok(())
    }
}

/// ============================================================================
/// PART 4: SSTABLES
/// ============================================================================

/// Formato do SSTable:
/// [u32 bloom_len][bloom_bytes]
/// [u32 meta_len][meta_bytes]
/// [records_blob...]
///
/// records_blob = repetição de [u32 record_len][record_bytes]
struct SStable {
    metadata: SstableMetadata,
    bloom_filter: Bloom<[u8]>,
    path: PathBuf,
}

impl SStable {
    fn create(dir_path: &Path, timestamp: u128, records: &[(String, LogRecord)]) -> Result<Self> {
        if records.is_empty() {
            return Err(LsmError::CompactionFailed(
                "Cannot create SSTable with empty records".to_string(),
            ));
        }

        let path = dir_path.join(format!("{}.sst", timestamp));
        let mut file = BufWriter::new(File::create(&path)?);

        // 1) Bloom filter (bloomfilter 3.x -> new_for_fp_rate retorna Result)
        let mut bloom = Bloom::<[u8]>::new_for_fp_rate(records.len(), 0.01)
            .map_err(|e| LsmError::CompactionFailed(e.to_string()))?;

        for (key, _) in records.iter() {
            bloom.set(key.as_bytes());
        }

        let bloom_bytes = bloom.into_bytes();

        // 2) Serializar records em ordem (já ordenados pela MemTable/BTreeMap)
        let mut records_blob = Vec::new();
        for (_key, record) in records.iter() {
            let record_bytes = serialize(record)?;
            let len = record_bytes.len() as u32;
            records_blob.extend_from_slice(&len.to_le_bytes());
            records_blob.extend_from_slice(&record_bytes);
        }

        let checksum = crc32fast::hash(&records_blob);

        // 3) Metadados
        let metadata = SstableMetadata {
            timestamp,
            min_key: records[0].0.clone(),
            max_key: records[records.len() - 1].0.clone(),
            record_count: records.len(),
            checksum,
        };
        let metadata_bytes = serialize(&metadata)?;

        // 4) Escrever arquivo
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

        // Reconstruct bloom filter from bytes for storage
        let bloom_filter = Bloom::<[u8]>::from_bytes(bloom_bytes)
            .map_err(|e| LsmError::CompactionFailed(e.to_string()))?;

        Ok(Self {
            metadata,
            bloom_filter,
            path,
        })
    }

    fn load(path: &Path) -> Result<Self> {
        let mut file = BufReader::new(File::open(path)?);
        let mut len_buf = [0u8; 4];

        // Bloom
        file.read_exact(&mut len_buf)?;
        let bloom_len = u32::from_le_bytes(len_buf) as usize;
        let mut bloom_data = vec![0u8; bloom_len];
        file.read_exact(&mut bloom_data)?;

        let bloom = Bloom::<[u8]>::from_bytes(bloom_data).map_err(|_| LsmError::InvalidSstable)?;

        // Metadata
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

    fn get(&self, key: &str) -> Result<Option<LogRecord>> {
        // Bloom check antes de tocar o disco (fora do arquivo)
        if !self.bloom_filter.check(key.as_bytes()) {
            debug!("Bloom negative: key={}", key);
            return Ok(None);
        }

        let mut file = BufReader::new(File::open(&self.path)?);
        let mut len_buf = [0u8; 4];

        // Pular bloom
        file.read_exact(&mut len_buf)?;
        let bloom_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(bloom_len as i64))?;

        // Pular metadata
        file.read_exact(&mut len_buf)?;
        let meta_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(meta_len as i64))?;

        // Varredura linear (fase 1). TODO: criar índice/sparse index para busca mais eficiente.
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

/// ============================================================================
/// PART 5: LSM ENGINE
/// ============================================================================

pub struct LsmEngine {
    memtable: Mutex<MemTable>,
    wal: WriteAheadLog,
    sstables: Mutex<Vec<SStable>>,
    dir_path: PathBuf,
    config: LsmConfig,
}

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

impl LsmEngine {
    pub fn new(config: LsmConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.data_dir)?;

        let wal = WriteAheadLog::new(&config.data_dir)?;
        let wal_records = wal.recover()?;

        // Carregar SSTables
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

        // Mais recente primeiro
        sstables.sort_by(|a, b| b.metadata.timestamp.cmp(&a.metadata.timestamp));

        // Rebuild MemTable a partir do WAL
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

        // 1) WAL
        self.wal.write_record(&record)?;

        // 2) MemTable
        let mut memtable = self.memtable.lock().unwrap();
        memtable.insert(record);

        // 3) Flush
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
        // 1) MemTable
        let memtable = self.memtable.lock().unwrap();
        if let Some(record) = memtable.get(key) {
            return Ok(if record.is_deleted {
                None
            } else {
                Some(record.value)
            });
        }
        drop(memtable);

        // 2) SSTables (mais recente -> mais antigo)
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
        info!("Starting memtable flush...");

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

        let cleared_count = memtable.clear();
        info!(
            "Memtable flushed: {} records, sstables={}",
            cleared_count,
            sstables.len()
        );

        drop(memtable);
        drop(sstables);

        self.wal.clear()?;

        // TODO: Size-Tiered Compaction
        // - agrupar SSTables pequenas por “tiers”
        // - merge mantendo apenas a versão mais recente por chave
        // - descartar tombstones antigos
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

/// ============================================================================
/// TESTS
/// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_memtable_ordering() {
        let mut mt = MemTable::new(1024);
        mt.insert(LogRecord::new("charlie".to_string(), b"3".to_vec()));
        mt.insert(LogRecord::new("alice".to_string(), b"1".to_vec()));
        mt.insert(LogRecord::new("bob".to_string(), b"2".to_vec()));

        let keys: Vec<_> = mt.iter_ordered().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["alice", "bob", "charlie"]);
    }

    #[test]
    fn test_set_and_get() -> Result<()> {
        let dir = tempdir()?;
        let config = LsmConfig {
            memtable_max_size: 4096,
            data_dir: dir.path().to_path_buf(),
        };

        let engine = LsmEngine::new(config)?;
        engine.set("key1".to_string(), b"value1".to_vec())?;

        let result = engine.get("key1")?;
        assert_eq!(result, Some(b"value1".to_vec()));
        Ok(())
    }

    #[test]
    fn test_delete_and_tombstone() -> Result<()> {
        let dir = tempdir()?;
        let config = LsmConfig {
            memtable_max_size: 4096,
            data_dir: dir.path().to_path_buf(),
        };

        let engine = LsmEngine::new(config)?;
        engine.set("key1".to_string(), b"value1".to_vec())?;
        engine.delete("key1".to_string())?;

        let result = engine.get("key1")?;
        assert_eq!(result, None);
        Ok(())
    }

    #[test]
    fn test_flush_creates_sstable() -> Result<()> {
        let dir = tempdir()?;
        let config = LsmConfig {
            memtable_max_size: 100, // pequeno para forçar flush
            data_dir: dir.path().to_path_buf(),
        };

        let engine = LsmEngine::new(config)?;
        engine.set(
            "key1".to_string(),
            b"value1_very_long_value_to_exceed_size".to_vec(),
        )?;

        let sstables = engine.sstables.lock().unwrap();
        assert!(!sstables.is_empty());
        Ok(())
    }
}
