pub struct StorageConfig {
    pub block_size: usize,
    pub block_cache_size_mb: usize,
    pub sparse_index_interval: usize,
    pub compaction_strategy: CompactionStrategy,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            block_size: 4096,
            block_cache_size_mb: 64,
            sparse_index_interval: 16,
            compaction_strategy: CompactionStrategy::SizeTiered,
        }
    }
}

// src/core/engine.rs
pub struct LsmConfig {
    pub dir_path: PathBuf,
    pub memtable_max_size: usize,
    pub storage: StorageConfig, // ✅ Composição
}
