use lsm_kv_store::{LsmConfig, LsmEngine, LsmError};
use tempfile::tempdir;

use std::fs::OpenOptions;

#[test]
fn restart_recovers_from_wal() {
    let dir = tempdir().unwrap();
    let cfg = LsmConfig {
        memtable_max_size: 1024 * 1024,
        data_dir: dir.path().to_path_buf(),
    };

    {
        let engine = LsmEngine::new(cfg.clone()).unwrap();
        engine.set("k1".to_string(), b"v1".to_vec()).unwrap();
    } // drop

    let engine = LsmEngine::new(cfg).unwrap();
    let v = engine.get("k1").unwrap().unwrap();
    assert_eq!(v, b"v1".to_vec());
}

#[test]
fn restart_after_flush_reads_sstable() {
    let dir = tempdir().unwrap();
    let cfg = LsmConfig {
        memtable_max_size: 64,
        data_dir: dir.path().to_path_buf(),
    };

    {
        let engine = LsmEngine::new(cfg.clone()).unwrap();
        for i in 0..50 {
            engine.set(format!("k{i}"), vec![b'x'; 20]).unwrap();
        }
    } // drop

    let engine = LsmEngine::new(cfg).unwrap();
    let v = engine.get("k1").unwrap().unwrap();
    assert!(!v.is_empty());
}

#[test]
fn tombstone_persists_across_restart() {
    let dir = tempdir().unwrap();
    let cfg = LsmConfig {
        memtable_max_size: 1024 * 1024,
        data_dir: dir.path().to_path_buf(),
    };

    {
        let engine = LsmEngine::new(cfg.clone()).unwrap();
        engine.set("k".to_string(), b"v".to_vec()).unwrap();
        engine.delete("k".to_string()).unwrap();
    } // drop

    let engine = LsmEngine::new(cfg).unwrap();
    assert!(engine.get("k").unwrap().is_none());
}

#[test]
fn wal_truncation_is_detected() {
    let dir = tempdir().unwrap();
    let cfg = LsmConfig {
        memtable_max_size: 1024 * 1024,
        data_dir: dir.path().to_path_buf(),
    };

    {
        let engine = LsmEngine::new(cfg.clone()).unwrap();
        engine.set("k1".to_string(), b"v1".to_vec()).unwrap();
    } // drop

    let wal_path = cfg.data_dir.join("wal.log");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&wal_path)
        .unwrap();

    let len = file.metadata().unwrap().len();
    assert!(len > 1);

    file.set_len(len - 1).unwrap(); // trunca 1 byte

    let res = LsmEngine::new(cfg);
    match res {
        Err(LsmError::WalCorruption) => {}
        Err(other) => panic!("expected WalCorruption, got: {other}"),
        Ok(_) => panic!("expected WalCorruption, got Ok"),
    }
}
