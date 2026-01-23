use crate::error::{LsmError, Result};
use crate::log_record::LogRecord;
use bincode::{deserialize, serialize};
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::debug;

pub struct WriteAheadLog {
    pub(crate) file: Mutex<BufWriter<File>>,
    pub(crate) path: PathBuf,
}

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
        Ok(records)
    }

    pub fn clear(&self) -> Result<()> {
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
