mod engine;
mod error;
mod log_record;
mod memtable;
mod sstable;
mod wal;

// Módulo API (condicional para não afetar lib pura)
#[cfg(feature = "api")]
pub mod api;

pub use crate::engine::{LsmConfig, LsmEngine};
pub use crate::error::{LsmError, Result};
pub use crate::log_record::LogRecord;
