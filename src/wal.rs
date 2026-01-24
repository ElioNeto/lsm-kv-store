use crate::error::{LsmError, Result};
use crate::log_record::LogRecord;
use bincode::{deserialize, serialize};
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::debug;

pub struct WriteAheadLog {
    pub(crate) file: Mutex<BufWriter<File>>,
    pub(crate) path: PathBuf,
}

const MAX_WAL_RECORD_BYTES: usize = 32 * 1024 * 1024; // 32MiB

impl WriteAheadLog {
    pub fn new(dir_path: &std::path::Path) -> Result<Self> {
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

    pub fn write_record(&self, record: &LogRecord) -> Result<()> {
        let serialized = serialize(record)?;
        let length = serialized.len() as u32;

        let mut writer = self.file.lock().unwrap();
        writer.write_all(&length.to_le_bytes())?;
        writer.write_all(&serialized)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;

        debug!("WAL persisted: key={}, ts={}", record.key, record.timestamp);
        Ok(())
    }

    pub fn recover(&self) -> Result<Vec<LogRecord>> {
        let mut records = Vec::new();
        let file = File::open(&self.path)?;
        let mut reader = BufReader::new(file);

        loop {
            // 1) Detecta EOF limpo vs truncamento no header de tamanho
            let buf = reader.fill_buf()?;
            if buf.is_empty() {
                break; // EOF limpo (boundary)
            }
            if buf.len() < 4 {
                return Err(LsmError::WalCorruption); // truncado no meio do length
            }

            // 2) Lê o tamanho do record (4 bytes)
            let mut lengthbuf = [0u8; 4];
            reader.read_exact(&mut lengthbuf)?;
            let length = u32::from_le_bytes(lengthbuf) as usize;

            if length == 0 || length > MAX_WAL_RECORD_BYTES {
                return Err(LsmError::WalCorruption);
            }

            // 3) Lê payload; se truncar aqui, é corrupção
            let mut buffer = vec![0u8; length];
            if let Err(e) = reader.read_exact(&mut buffer) {
                if e.kind() == io::ErrorKind::UnexpectedEof {
                    return Err(LsmError::WalCorruption);
                }
                return Err(e.into());
            }

            let record: LogRecord = deserialize(&buffer).map_err(|_| LsmError::WalCorruption)?;
            records.push(record);
        }

        Ok(records)
    }
    pub fn clear(&self) -> Result<()> {
        let mut guard = self.file.lock().unwrap();
        guard.flush()?;
        guard.get_ref().sync_all()?;

        let truncfile = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        truncfile.sync_all()?;

        let appendfile = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        *guard = BufWriter::new(appendfile);
        Ok(())
    }
}
