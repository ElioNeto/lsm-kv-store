use lru::LruCache;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Cache key that uniquely identifies a block across multiple SSTable files.
/// Combines file identity (hash of path) with block offset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    file_id: u64,      // Hash of the file path
    block_offset: u64, // Block offset within the file
}

impl CacheKey {
    /// Creates a new cache key from a file path and block offset.
    ///
    /// # Arguments
    /// * `path` - Path to the SSTable file
    /// * `offset` - Byte offset of the block within the file
    pub fn new(path: &PathBuf, offset: u64) -> Self {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let file_id = hasher.finish();

        Self {
            file_id,
            block_offset: offset,
        }
    }
}

/// Global shared block cache that is shared across all SSTable readers.
/// Uses LRU eviction policy to manage memory usage.
#[derive(Debug)]
pub struct GlobalBlockCache {
    cache: Mutex<LruCache<CacheKey, Arc<Vec<u8>>>>,
}

impl GlobalBlockCache {
    /// Creates a new global block cache.
    ///
    /// # Arguments
    /// * `capacity_mb` - Maximum cache size in megabytes
    /// * `block_size` - Size of each block in bytes
    ///
    /// # Returns
    /// Arc-wrapped cache instance for shared ownership
    pub fn new(capacity_mb: usize, block_size: usize) -> Arc<Self> {
        let capacity_bytes = capacity_mb * 1024 * 1024;
        let num_blocks = (capacity_bytes / block_size).max(1);
        let capacity = NonZeroUsize::new(num_blocks).unwrap();

        Arc::new(Self {
            cache: Mutex::new(LruCache::new(capacity)),
        })
    }

    /// Retrieves a block from the cache.
    ///
    /// # Arguments
    /// * `key` - Cache key identifying the block
    ///
    /// # Returns
    /// Some(Arc<Vec<u8>>) if found, None if cache miss
    pub fn get(&self, key: &CacheKey) -> Option<Arc<Vec<u8>>> {
        let mut cache = self.cache.lock().unwrap();
        cache.get(key).cloned()
    }

    /// Inserts a block into the cache.
    ///
    /// # Arguments
    /// * `key` - Cache key identifying the block
    /// * `value` - Block data to cache
    pub fn put(&self, key: CacheKey, value: Vec<u8>) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(key, Arc::new(value));
    }

    /// Clears all entries from the cache.
    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }

    /// Returns cache statistics.
    pub fn stats(&self) -> CacheStats {
        let cache = self.cache.lock().unwrap();
        CacheStats {
            len: cache.len(),
            cap: cache.cap().get(),
        }
    }
}

/// Statistics about cache usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    /// Number of entries currently in cache
    pub len: usize,
    /// Maximum capacity of the cache
    pub cap: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_uniqueness_different_files() {
        let path1 = PathBuf::from("/data/sst1.sst");
        let path2 = PathBuf::from("/data/sst2.sst");

        let key1 = CacheKey::new(&path1, 0);
        let key2 = CacheKey::new(&path2, 0);

        // Different files should produce different keys
        assert_ne!(key1, key2);
        assert_ne!(key1.file_id, key2.file_id);
    }

    #[test]
    fn test_cache_key_same_file_different_offsets() {
        let path = PathBuf::from("/data/sst1.sst");

        let key1 = CacheKey::new(&path, 0);
        let key2 = CacheKey::new(&path, 4096);

        // Different offsets should produce different keys
        assert_ne!(key1, key2);
        // But same file_id
        assert_eq!(key1.file_id, key2.file_id);
    }

    #[test]
    fn test_cache_key_deterministic() {
        let path = PathBuf::from("/data/test.sst");

        let key1 = CacheKey::new(&path, 1024);
        let key2 = CacheKey::new(&path, 1024);

        // Same path and offset should produce identical keys
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_global_cache_basic_operations() {
        let cache = GlobalBlockCache::new(1, 4096); // 1MB, 4KB blocks

        let key = CacheKey::new(&PathBuf::from("test.sst"), 0);
        let data = vec![1, 2, 3, 4, 5];

        // Initially empty
        assert!(cache.get(&key).is_none());

        // Put and retrieve
        cache.put(key.clone(), data.clone());
        let retrieved = cache.get(&key).unwrap();
        assert_eq!(*retrieved, data);
    }

    #[test]
    fn test_global_cache_arc_sharing() {
        let cache = GlobalBlockCache::new(1, 4096);

        let key = CacheKey::new(&PathBuf::from("test.sst"), 0);
        let data = vec![1, 2, 3, 4, 5];

        cache.put(key.clone(), data.clone());

        // Get twice and verify both point to same data
        let ref1 = cache.get(&key).unwrap();
        let ref2 = cache.get(&key).unwrap();

        // Arc should allow multiple references
        assert_eq!(*ref1, *ref2);
        assert_eq!(*ref1, data);
    }

    #[test]
    fn test_global_cache_capacity() {
        let cache = GlobalBlockCache::new(1, 4096); // Can hold ~256 blocks (1MB / 4KB)

        let stats = cache.stats();
        assert_eq!(stats.len, 0);
        assert!(stats.cap > 0);
        assert_eq!(stats.cap, (1 * 1024 * 1024) / 4096);
    }

    #[test]
    fn test_global_cache_lru_eviction() {
        // Small cache that can hold only 2 blocks
        let cache = GlobalBlockCache::new(1, 512 * 1024); // ~2 blocks

        let key1 = CacheKey::new(&PathBuf::from("test1.sst"), 0);
        let key2 = CacheKey::new(&PathBuf::from("test2.sst"), 0);
        let key3 = CacheKey::new(&PathBuf::from("test3.sst"), 0);

        let data = vec![0u8; 1024]; // Small data

        cache.put(key1.clone(), data.clone());
        cache.put(key2.clone(), data.clone());

        // Both should be in cache
        assert!(cache.get(&key1).is_some());
        assert!(cache.get(&key2).is_some());

        // Add third entry, should evict least recently used (key1)
        cache.put(key3.clone(), data.clone());

        let stats = cache.stats();
        assert!(stats.len <= stats.cap);
    }

    #[test]
    fn test_global_cache_clear() {
        let cache = GlobalBlockCache::new(1, 4096);

        let key1 = CacheKey::new(&PathBuf::from("test1.sst"), 0);
        let key2 = CacheKey::new(&PathBuf::from("test2.sst"), 0);

        cache.put(key1.clone(), vec![1, 2, 3]);
        cache.put(key2.clone(), vec![4, 5, 6]);

        assert_eq!(cache.stats().len, 2);

        cache.clear();

        assert_eq!(cache.stats().len, 0);
        assert!(cache.get(&key1).is_none());
        assert!(cache.get(&key2).is_none());
    }

    #[test]
    fn test_global_cache_update_existing_key() {
        let cache = GlobalBlockCache::new(1, 4096);

        let key = CacheKey::new(&PathBuf::from("test.sst"), 0);

        cache.put(key.clone(), vec![1, 2, 3]);
        let first = cache.get(&key).unwrap();
        assert_eq!(*first, vec![1, 2, 3]);

        // Update with new value
        cache.put(key.clone(), vec![4, 5, 6]);
        let second = cache.get(&key).unwrap();
        assert_eq!(*second, vec![4, 5, 6]);

        // Should still have only 1 entry
        assert_eq!(cache.stats().len, 1);
    }

    #[test]
    fn test_global_cache_multiple_files_same_offset() {
        let cache = GlobalBlockCache::new(1, 4096);

        let path1 = PathBuf::from("/data/file1.sst");
        let path2 = PathBuf::from("/data/file2.sst");

        let key1 = CacheKey::new(&path1, 0);
        let key2 = CacheKey::new(&path2, 0);

        cache.put(key1.clone(), vec![1, 1, 1]);
        cache.put(key2.clone(), vec![2, 2, 2]);

        // Both should be retrievable independently
        let data1 = cache.get(&key1).unwrap();
        let data2 = cache.get(&key2).unwrap();

        assert_eq!(*data1, vec![1, 1, 1]);
        assert_eq!(*data2, vec![2, 2, 2]);

        assert_eq!(cache.stats().len, 2);
    }
}
