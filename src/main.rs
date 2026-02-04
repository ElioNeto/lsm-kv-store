use lsm_kv_store::{LsmConfig, LsmEngine};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = LsmConfig::builder()
        .dir_path("/var/lib/lsm_kv_store/data")
        .build()?;

    let _engine = LsmEngine::new(config)?;
    Ok(())
}
