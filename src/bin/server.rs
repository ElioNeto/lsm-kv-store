use lsm_kv_store::{LsmConfig, LsmEngine};
use std::path::PathBuf;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Configurar tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    // Configurar engine
    let config = LsmConfig {
        memtable_max_size: 4 * 1024 * 1024, // 4MB
        data_dir: PathBuf::from("./.lsm_data"),
    };

    let engine = LsmEngine::new(config)
        .expect("Failed to initialize LSM Engine");

    // Iniciar servidor HTTP
    lsm_kv_store::api::start_server(engine, "127.0.0.1", 8080).await
}
