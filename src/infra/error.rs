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

    #[error("Invalid SSTable format: {0}")]
    InvalidSstableFormat(String),

    #[error("Corrupted data: {0}")]
    CorruptedData(String),

    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),

    #[error("Compaction failed: {0}")]
    CompactionFailed(String),

    #[error("WAL corruption detected")]
    WalCorruption,

    #[error("Serialization failed: {0}")]
    SerializationFailed(String),

    #[error("Deserialization failed: {0}")]
    DeserializationFailed(String),

    #[error("Concurrent modification detected")]
    ConcurrentModification,

    #[error("Key not found")]
    NotFound,

    // Configuration validation errors
    #[error("Invalid block size: {0}")]
    InvalidBlockSize(String),

    #[error("Invalid cache size: {0}")]
    InvalidCacheSize(String),

    #[error("Invalid sparse index interval: {0}")]
    InvalidIndexInterval(String),

    #[error("Invalid Bloom filter false positive rate: {0}")]
    InvalidBloomRate(String),

    #[error("Invalid memtable size: {0}")]
    InvalidMemtableSize(String),

    #[error("Configuration validation failed: {0}")]
    ConfigValidation(String),
}

pub type Result<T> = std::result::Result<T, LsmError>;
