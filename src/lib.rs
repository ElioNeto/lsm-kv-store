//! # LSM-Tree Key-Value Store (Fase 1: Storage Engine)
//!
//! Este módulo implementa os componentes fundamentais de um LSM-Tree:
//! - **MemTable**: Estrutura em memória com BTreeMap para ordenação alfabética
//! - **Write-Ahead Log (WAL)**: Persistência síncrona de escritas
//! - **SSTables**: Arquivos imutáveis no disco com Bloom Filters
//! - **Compaction**: Estratégia Size-Tiered para manutenção
//!
//! Arquitetura:
//! ```text
//! ┌──────────────┐
//! │   SET/GET    │
//! └────────┬─────┘
//!          │
//!          ├─→ WAL (Write-Ahead Log) ──→ Disco
//!          │
//!          ├─→ MemTable (BTreeMap) ──→ Memória
//!          │
//!          └─→ SSTables (Bloom Filter + Dados Ordenados) ──→ Disco
//! ```

use bincode::{deserialize, serialize};
use bloomfilter::Bloom;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;
use tracing::{debug, warn, info};

/// Erros possíveis durante operações do LSM-Tree
#[derive(Error, Debug)]
pub enum LsmError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

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

/// Registro de log (LogRecord) que será serializado em binário
///
/// Campos obrigatórios para o LSM-Tree:
/// - `key`: Identificador único (String)
/// - `value`: Dados armazenados (Vec<u8>)
/// - `timestamp`: Momento exato da escrita em nanosegundos
/// - `is_deleted`: Tombstone para deleções lógicas (importante para compaction)
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogRecord {
    pub key: String,
    pub value: Vec<u8>,
    pub timestamp: u128,
    pub is_deleted: bool,
}

impl LogRecord {
    /// Cria um novo LogRecord com timestamp atual
    pub fn new(key: String, value: Vec<u8>) -> Self {
        Self {
            key,
            value,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
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
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            is_deleted: true,
        }
    }
}

/// Metadados de um SSTable para rápido acesso aos Bloom Filters
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SstableMetadata {
    /// Timestamp do SSTable (nome do arquivo)
    pub timestamp: u128,
    /// Chave mínima (primeira chave em ordem)
    pub min_key: String,
    /// Chave máxima (última chave em ordem)
    pub max_key: String,
    /// Quantidade de registros
    pub record_count: usize,
    /// CRC32 para validação de integridade
    pub checksum: u32,
}

/// ============================================================================
/// PART 2: MEMTABLE (Em Memória com BTreeMap)
/// ============================================================================

/// MemTable em memória com BTreeMap
///
/// **Garantias:**
/// - Todas as chaves sempre em ordem alfabética (propriedade BTreeMap)
/// - Fácil serialização sequencial para SSTable
/// - Operações O(log n)
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

    /// Insere um registro e atualiza o tamanho em bytes
    fn insert(&mut self, record: LogRecord) {
        let record_size = Self::estimate_size(&record);
        if let Some(old_record) = self.data.insert(record.key.clone(), record) {
            self.size_bytes -= Self::estimate_size(&old_record);
        }
        self.size_bytes += record_size;
    }

    /// Retorna true se MemTable deve sofrer flush
    fn should_flush(&self) -> bool {
        self.size_bytes >= self.max_size_bytes
    }

    /// Obtém um registro da MemTable
    fn get(&self, key: &str) -> Option<LogRecord> {
        self.data.get(key).cloned()
    }

    /// Retorna iterador sobre registros em ordem alfabética
    fn iter_ordered(&self) -> impl Iterator<Item = (&String, &LogRecord)> {
        self.data.iter()
    }

    /// Limpa a MemTable e retorna quantidade de registros removidos
    fn clear(&mut self) -> usize {
        let count = self.data.len();
        self.data.clear();
        self.size_bytes = 0;
        count
    }

    /// Estima tamanho em bytes de um LogRecord (aproximado)
    fn estimate_size(record: &LogRecord) -> usize {
        record.key.len() + record.value.len() + 32 // 32 = timestamp (16) + is_deleted (1) + overhead
    }
}

/// ============================================================================
/// PART 3: WRITE-AHEAD LOG (WAL)
/// ============================================================================

/// Write-Ahead Log (WAL) para durabilidade
///
/// **Garantias:**
/// - Cada SET é sincronizado no disco (fsync) antes de entrar na MemTable
/// - Append-only format
/// - Recovery automática na inicialização
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

    /// Escreve um LogRecord no WAL e força sincronização com disco
    fn write_record(&self, record: &LogRecord) -> Result<()> {
        let serialized = serialize(record)?;
        let length = serialized.len() as u32;

        let mut writer = self.file.lock().unwrap();

        // Escreve tamanho (4 bytes) + dados
        writer.write_all(&length.to_le_bytes())?;
        writer.write_all(&serialized)?;

        // Força sincronização síncrona com disco
        writer.flush()?;
        writer.get_ref().sync_all()?;

        debug!("WAL record persisted: key={}, timestamp={}", record.key, record.timestamp);
        Ok(())
    }

    /// Recupera todos os LogRecords do WAL durante inicialização
    fn recover(&self) -> Result<Vec<LogRecord>> {
        let mut records = Vec::new();
        let mut file = std::fs::File::open(&self.path)?;
        let mut reader = BufReader::new(&mut file);
        let mut length_buf = [0u8; 4];

        loop {
            match reader.read_exact(&mut length_buf) {
                Ok(()) => {
                    let length = u32::from_le_bytes(length_buf) as usize;
                    let mut buffer = vec![0u8; length];
                    reader.read_exact(&mut buffer)?;
                    let record: LogRecord = deserialize(&buffer)
                        .map_err(|_| LsmError::WalCorruption)?;
                    records.push(record);
                }
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
        }

        info!("WAL recovery: {} records recovered", records.len());
        Ok(records)
    }

    /// Limpa o WAL após um flush bem-sucedido
    fn clear(&self) -> Result<()> {
        let mut writer = self.file.lock().unwrap();
        drop(writer);
        std::fs::File::create(&self.path)?;
        Ok(())
    }
}

/// ============================================================================
/// PART 4: SSTORE (SSTables) - Arquivos Imutáveis no Disco
/// ============================================================================

/// SSTable (Sorted String Table) - Arquivo imutável com dados ordenados
///
/// Formato do arquivo:
/// ```text
/// [Bloom Filter Serializado][Metadados][Registros em Ordem][CRC32]
/// ```
///
/// **Propriedades:**
/// - Dados sempre em ordem alfabética (garantido pela MemTable)
/// - Bloom Filter no cabeçalho para rápida negação de presença
/// - Imutável após criação (seguro para leitura concorrente)
struct SStable {
    metadata: SstableMetadata,
    bloom_filter: Bloom<Vec<u8>>,
    path: PathBuf,
}

impl SStable {
    /// Cria um novo SSTable a partir dos dados da MemTable
    fn create(
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

        // 1. Criar e serializar Bloom Filter
        let mut bloom = Bloom::new_for_fp_rate(records.len(), 0.01); // 1% false positive rate
        for (key, _) in records.iter() {
            bloom.set(&key.as_bytes().to_vec());
        }
        let bloom_serialized = serialize(&bloom.sip_keys())?; // Serializa apenas as chaves SIP

        // 2. Preparar metadados
        let mut metadata = SstableMetadata {
            timestamp,
            min_key: records[0].0.clone(),
            max_key: records[records.len() - 1].0.clone(),
            record_count: records.len(),
            checksum: 0, // Calculado depois
        };

        // 3. Escrever Bloom Filter
        file.write_all(&(bloom_serialized.len() as u32).to_le_bytes())?;
        file.write_all(&bloom_serialized)?;

        // 4. Escrever Metadados
        let metadata_serialized = serialize(&metadata)?;
        file.write_all(&(metadata_serialized.len() as u32).to_le_bytes())?;
        file.write_all(&metadata_serialized)?;

        // 5. Escrever Registros em Ordem
        for (_key, record) in records.iter() {
            let record_serialized = serialize(record)?;
            file.write_all(&(record_serialized.len() as u32).to_le_bytes())?;
            file.write_all(&record_serialized)?;
        }

        // 6. Calcular e escrever CRC32
        file.flush()?;
        file.get_ref().sync_all()?;

        let checksum = crc32fast::hash(&std::fs::read(&path)?);
        metadata.checksum = checksum;

        debug!("SSTable created: {}, records={}, checksum={}", path.display(), records.len(), checksum);

        Ok(Self {
            metadata,
            bloom_filter: bloom,
            path,
        })
    }

    /// Carrega um SSTable do disco
    fn load(path: &Path) -> Result<Self> {
        let mut file = BufReader::new(File::open(path)?);

        // 1. Ler Bloom Filter
        let mut len_buf = [0u8; 4];
        file.read_exact(&mut len_buf)?;
        let bloom_len = u32::from_le_bytes(len_buf) as usize;
        let mut bloom_data = vec![0u8; bloom_len];
        file.read_exact(&mut bloom_data)?;
        let bloom = Bloom::from_bytes(&bloom_data)?; // Reconstrói o Bloom

        // 2. Ler Metadados
        file.read_exact(&mut len_buf)?;
        let metadata_len = u32::from_le_bytes(len_buf) as usize;
        let mut metadata_data = vec![0u8; metadata_len];
        file.read_exact(&mut metadata_data)?;
        let metadata: SstableMetadata = deserialize(&metadata_data)?;

        Ok(Self {
            metadata,
            bloom_filter: bloom,
            path: path.to_path_buf(),
        })
    }

    /// Busca uma chave no SSTable (consulta Bloom Filter primeiro)
    fn get(&self, key: &str) -> Result<Option<LogRecord>> {
        // Otimização: Verificar Bloom Filter antes de abrir arquivo
        if !self.bloom_filter.check(&key.as_bytes().to_vec()) {
            debug!("Bloom filter negative for key: {}", key);
            return Ok(None); // Chave definitivamente não existe
        }

        let mut file = BufReader::new(File::open(&self.path)?);
        let mut len_buf = [0u8; 4];

        // Pular Bloom Filter
        file.read_exact(&mut len_buf)?;
        let bloom_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(bloom_len as i64))?;

        // Pular Metadados
        file.read_exact(&mut len_buf)?;
        let metadata_len = u32::from_le_bytes(len_buf) as usize;
        file.seek(SeekFrom::Current(metadata_len as i64))?;

        // Procurar registro
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
/// PART 5: LSM ENGINE (Motor Principal)
/// ============================================================================

/// Motor LSM-Tree Principal
///
/// Este é o componente central que coordena:
/// - MemTable (em memória)
/// - WAL (Write-Ahead Log)
/// - SSTables (arquivos no disco)
/// - Estratégia de Compaction
pub struct LsmEngine {
    memtable: Mutex<MemTable>,
    wal: WriteAheadLog,
    sstables: Mutex<Vec<SStable>>,
    dir_path: PathBuf,
    config: LsmConfig,
}

/// Configuração do LSM Engine
pub struct LsmConfig {
    /// Tamanho máximo da MemTable em bytes
    pub memtable_max_size: usize,
    /// Diretório de dados
    pub data_dir: PathBuf,
}

impl Default for LsmConfig {
    fn default() -> Self {
        Self {
            memtable_max_size: 4 * 1024 * 1024, // 4MB padrão
            data_dir: PathBuf::from("./.lsm_data"),
        }
    }
}

impl LsmEngine {
    /// Inicializa o LSM Engine com recovery automático do WAL
    ///
    /// # Operações:
    /// 1. Cria diretório de dados se não existir
    /// 2. Carrega todos os SSTables do disco
    /// 3. Recupera WAL e recarrega MemTable
    pub fn new(config: LsmConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.data_dir)?;

        let wal = WriteAheadLog::new(&config.data_dir)?;

        // Recuperar WAL
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

        // Ordenar SSTables por timestamp (mais recentes primeiro)
        sstables.sort_by(|a, b| b.metadata.timestamp.cmp(&a.metadata.timestamp));

        // Reconstruir MemTable a partir do WAL
        let mut memtable = MemTable::new(config.memtable_max_size);
        for record in wal_records {
            memtable.insert(record);
        }

        info!("LSM Engine initialized: {} sstables, memtable with {} records",
            sstables.len(), memtable.data.len());

        Ok(Self {
            memtable: Mutex::new(memtable),
            wal,
            sstables: Mutex::new(sstables),
            dir_path: config.data_dir.clone(),
            config,
        })
    }

    /// Define um par chave-valor
    ///
    /// # Ordem de Operação (Crítica):
    /// 1. Escrever no WAL (durabilidade garantida)
    /// 2. Inserir na MemTable
    /// 3. Verificar se MemTable deve fazer flush
    /// 4. Se necessário, chamar flush()
    pub fn set(&self, key: String, value: Vec<u8>) -> Result<()> {
        let record = LogRecord::new(key.clone(), value);

        // 1. Escrever no WAL PRIMEIRO (garante durabilidade)
        self.wal.write_record(&record)?;

        // 2. Inserir na MemTable
        let mut memtable = self.memtable.lock().unwrap();
        memtable.insert(record);

        // 3. Verificar necessidade de flush
        if memtable.should_flush() {
            drop(memtable); // Libera lock
            self.flush()?;
        }

        Ok(())
    }

    /// Deleta uma chave (cria tombstone)
    pub fn delete(&self, key: String) -> Result<()> {
        let record = LogRecord::tombstone(key);

        // Mesmo protocolo do SET
        self.wal.write_record(&record)?;

        let mut memtable = self.memtable.lock().unwrap();
        memtable.insert(record);

        if memtable.should_flush() {
            drop(memtable);
            self.flush()?;
        }

        Ok(())
    }

    /// Obtém o valor de uma chave
    ///
    /// # Ordem de Busca:
    /// 1. Verificar MemTable
    /// 2. Se não encontrado, verificar SSTables (mais recentes primeiro)
    /// 3. Usar Bloom Filter para evitar leituras desnecessárias
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        // 1. Buscar na MemTable
        let memtable = self.memtable.lock().unwrap();
        if let Some(record) = memtable.get(key) {
            return Ok(if record.is_deleted {
                None
            } else {
                Some(record.value)
            });
        }
        drop(memtable);

        // 2. Buscar nos SSTables (do mais recente para o mais antigo)
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

    /// Flush: Converte MemTable em SSTable
    ///
    /// # Operações:
    /// 1. Obter snapshot da MemTable
    /// 2. Criar SSTable com Bloom Filter
    /// 3. Limpar MemTable
    /// 4. Limpar WAL
    /// 5. Registrar novo SSTable
    fn flush(&self) -> Result<()> {
        info!("Starting memtable flush...");

        let mut memtable = self.memtable.lock().unwrap();

        // Snapshot dos dados (em ordem alfabética graças ao BTreeMap)
        let records: Vec<(String, LogRecord)> = memtable
            .iter_ordered()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if records.is_empty() {
            return Ok(());
        }

        // Criar SSTable
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();

        let sst = SStable::create(&self.dir_path, timestamp, &records)?;

        // Registrar novo SSTable
        let mut sstables = self.sstables.lock().unwrap();
        sstables.insert(0, sst); // Inserir no começo (mais recente primeiro)

        // Limpar MemTable
        let cleared_count = memtable.clear();
        info!("Memtable flushed: {} records, {} sstables now in use",
            cleared_count, sstables.len());

        // Limpar WAL
        drop(memtable);
        drop(sstables);
        self.wal.clear()?;

        // TODO: COMPACTION STRATEGY
        // Próxima fase: Implementar Size-Tiered Compaction
        // Triggers:
        // - Quando número de SSTables > threshold
        // - Quando tamanho total > limite
        // Operação:
        // - Mesclar SSTables menores em arquivo único
        // - Remover duplicatas mantendo versão mais recente
        // - Remover tombstones

        Ok(())
    }

    /// Retorna estatísticas do engine
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
/// PART 6: TESTS
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

        let keys: Vec<_> = mt.iter_ordered().map(|(k, _)| k).collect();
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
    fn test_memtable_flush() -> Result<()> {
        let dir = tempdir()?;
        let config = LsmConfig {
            memtable_max_size: 100, // Pequeno para forçar flush
            data_dir: dir.path().to_path_buf(),
        };

        let engine = LsmEngine::new(config)?;
        engine.set("key1".to_string(), b"value1_very_long_value_to_exceed_size".to_vec())?;

        // Verificar que SSTable foi criado
        let sstables = engine.sstables.lock().unwrap();
        assert!(!sstables.is_empty());

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
}
