use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LsmConfig {
    pub core: CoreConfig,
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreConfig {
    pub dir_path: PathBuf,
    pub memtable_max_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub block_size: usize,
    pub block_cache_size_mb: usize,
    pub sparse_index_interval: usize,
    pub bloom_false_positive_rate: f64,
}

impl Default for LsmConfig {
    fn default() -> Self {
        Self {
            core: CoreConfig::default(),
            storage: StorageConfig::default(),
        }
    }
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            dir_path: PathBuf::from("./.lsmdata"),
            memtable_max_size: 4 * 1024 * 1024,
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            block_size: 4096,
            block_cache_size_mb: 64,
            sparse_index_interval: 16,
            bloom_false_positive_rate: 0.01,
        }
    }
}

impl LsmConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builder() -> LsmConfigBuilder {
        LsmConfigBuilder::default()
    }
}

#[derive(Default)]
pub struct LsmConfigBuilder {
    dir_path: Option<PathBuf>,
    memtable_max_size: Option<usize>,
    block_size: Option<usize>,
    block_cache_size_mb: Option<usize>,
    sparse_index_interval: Option<usize>,
    bloom_false_positive_rate: Option<f64>,
}

impl LsmConfigBuilder {
    pub fn dir_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.dir_path = Some(path.into());
        self
    }

    pub fn memtable_max_size(mut self, size: usize) -> Self {
        self.memtable_max_size = Some(size);
        self
    }

    pub fn block_size(mut self, size: usize) -> Self {
        self.block_size = Some(size);
        self
    }

    pub fn block_cache_size_mb(mut self, size: usize) -> Self {
        self.block_cache_size_mb = Some(size);
        self
    }

    pub fn sparse_index_interval(mut self, interval: usize) -> Self {
        self.sparse_index_interval = Some(interval);
        self
    }

    pub fn bloom_false_positive_rate(mut self, rate: f64) -> Self {
        self.bloom_false_positive_rate = Some(rate);
        self
    }

    pub fn build(self) -> LsmConfig {
        let defaults = LsmConfig::default();

        LsmConfig {
            core: CoreConfig {
                dir_path: self.dir_path.unwrap_or(defaults.core.dir_path),
                memtable_max_size: self
                    .memtable_max_size
                    .unwrap_or(defaults.core.memtable_max_size),
            },
            storage: StorageConfig {
                block_size: self.block_size.unwrap_or(defaults.storage.block_size),
                block_cache_size_mb: self
                    .block_cache_size_mb
                    .unwrap_or(defaults.storage.block_cache_size_mb),
                sparse_index_interval: self
                    .sparse_index_interval
                    .unwrap_or(defaults.storage.sparse_index_interval),
                bloom_false_positive_rate: self
                    .bloom_false_positive_rate
                    .unwrap_or(defaults.storage.bloom_false_positive_rate),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LsmConfig::default();
        assert_eq!(config.core.memtable_max_size, 4 * 1024 * 1024);
        assert_eq!(config.storage.block_size, 4096);
        assert_eq!(config.storage.block_cache_size_mb, 64);
    }

    #[test]
    fn test_builder() {
        let config = LsmConfig::builder()
            .dir_path("/tmp/test")
            .memtable_max_size(8 * 1024 * 1024)
            .block_size(8192)
            .block_cache_size_mb(128)
            .build();

        assert_eq!(config.core.dir_path, PathBuf::from("/tmp/test"));
        assert_eq!(config.core.memtable_max_size, 8 * 1024 * 1024);
        assert_eq!(config.storage.block_size, 8192);
        assert_eq!(config.storage.block_cache_size_mb, 128);
    }

    #[test]
    fn test_partial_builder() {
        let config = LsmConfig::builder().dir_path("/custom/path").build();

        assert_eq!(config.core.dir_path, PathBuf::from("/custom/path"));
        assert_eq!(config.core.memtable_max_size, 4 * 1024 * 1024);
        assert_eq!(config.storage.block_size, 4096);
    }
}
