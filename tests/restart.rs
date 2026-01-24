use lsm_kv_store::{LsmConfig, LsmEngine};
use tempfile::tempdir;

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
    }

    {
        let engine = LsmEngine::new(cfg).unwrap();
        let v = engine.get("k1").unwrap().unwrap();
        assert_eq!(v, b"v1".to_vec());
    }
}

#[test]
fn restart_after_flush_reads_sstable() {
    let dir = tempdir().unwrap();
    // pequeno para disparar flush automático
    let cfg = LsmConfig {
        memtable_max_size: 64,
        data_dir: dir.path().to_path_buf(),
    };

    {
        let engine = LsmEngine::new(cfg.clone()).unwrap();
        for i in 0..50 {
            engine.set(format!("k{i}"), vec![b'x'; 20]).unwrap();
        }
        // se o flush ocorrer, o engine chama wal.clear() e cria *.sst [file:6]
    }

    {
        let engine = LsmEngine::new(cfg).unwrap();
        let v = engine.get("k1").unwrap().unwrap();
        assert!(!v.is_empty());
    }
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
    }

    {
        let engine = LsmEngine::new(cfg).unwrap();
        assert!(engine.get("k").unwrap().is_none());
    }
}
