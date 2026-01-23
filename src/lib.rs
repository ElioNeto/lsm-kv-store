//! LSM-Tree KV Store – Fase 1 (Storage Engine)

mod engine;
mod error;
mod log_record;
mod memtable;
mod sstable;
mod wal;

pub use crate::engine::{LsmConfig, LsmEngine};
pub use crate::error::{LsmError, Result};
pub use crate::log_record::LogRecord;
