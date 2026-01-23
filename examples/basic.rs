use lsm_kv_store::{LsmConfig, LsmEngine};
use tempfile::tempdir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let cfg = LsmConfig {
        memtable_max_size: 4 * 1024,
        data_dir: dir.path().to_path_buf(),
    };

    let db = LsmEngine::new(cfg)?;
    db.set("hello".to_string(), b"world".to_vec())?;

    let v = db.get("hello")?;
    println!("GET hello = {:?}", v);

    Ok(())
}
