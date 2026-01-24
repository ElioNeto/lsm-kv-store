use bincode;
use std::io;
use std::time::SystemTimeError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LsmError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("System time error: {0}")]
    Time(#[from] SystemTimeError),

    #[error("Lock poisoned: {0}")]
    LockPoisoned(&'static str),

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
