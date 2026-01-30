use crate::infra::codec::decode;
use crate::infra::error::{LsmError, Result};
use crate::core::log_record::LogRecord;
use crate::core::memtable::MemTable;
use crate::storage::sstable::SStable;
use crate::storage::wal::WriteAheadLog;

// ... (dentro do método get, adicione a anotação de tipo)
pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
    // ...
    let sstables = self.sstables_lock()?;
    for sst in sstables.iter() {
        // CORREÇÃO: Adicionamos tipo explícito aqui
        if let Some(record): Option<LogRecord> = sst.get(key)? {
            return Ok(if record.is_deleted {
                None
            } else {
                Some(record.value)
            });
        }
    }
    Ok(None)
}