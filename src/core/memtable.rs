use crate::core::log_record::LogRecord;
use std::collections::BTreeMap;

pub struct MemTable {
    pub(crate) data: BTreeMap<String, LogRecord>,
    pub(crate) size_bytes: usize,
    pub(crate) max_size_bytes: usize,
}

impl MemTable {
    pub fn new(max_size_bytes: usize) -> Self {
        Self {
            data: BTreeMap::new(),
            size_bytes: 0,
            max_size_bytes,
        }
    }

    pub fn insert(&mut self, record: LogRecord) {
        let record_size = Self::estimate_size(&record);
        if let Some(old_record) = self.data.insert(record.key.clone(), record) {
            self.size_bytes = self
                .size_bytes
                .saturating_sub(Self::estimate_size(&old_record));
        }
        self.size_bytes += record_size;
    }

    pub fn should_flush(&self) -> bool {
        self.size_bytes >= self.max_size_bytes
    }

    pub fn get(&self, key: &str) -> Option<LogRecord> {
        self.data.get(key).cloned()
    }

    pub fn iter_ordered(&self) -> impl Iterator<Item = (&String, &LogRecord)> {
        self.data.iter()
    }

    pub fn clear(&mut self) -> usize {
        let count = self.data.len();
        self.data.clear();
        self.size_bytes = 0;
        count
    }

    fn estimate_size(record: &LogRecord) -> usize {
        record.key.len() + record.value.len() + 32
    }
}
