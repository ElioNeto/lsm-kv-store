pub mod core;
pub mod features;
pub mod infra;
pub mod storage;

#[cfg(feature = "api")]
pub mod api;

// Re-exports para manter compatibilidade onde necessário
pub use crate::core::engine::{LsmConfig, LsmEngine};
pub use crate::core::log_record::LogRecord;
pub use crate::features::{FeatureClient, FeatureFlag, Features};
pub use crate::infra::error::{LsmError, Result};
