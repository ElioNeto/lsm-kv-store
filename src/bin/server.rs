use lsm_kv_store::{LsmConfig, LsmEngine};
use std::env;
use std::io;
use std::path::PathBuf;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    println!("╔═══════════════════════════════════════════════════════╗");
    println!("║         LSM-Tree REST API Server                      ║");
    println!("╚═══════════════════════════════════════════════════════╝\n");

    let data_dir = env::var("DATA_DIR").unwrap_or_else(|_| "./.lsm_data".to_string());

    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);

    let config = LsmConfig::builder()
        .dir_path(PathBuf::from(data_dir))
        .memtable_max_size(4 * 1024 * 1024)
        .build();

    match config.core.dir_path.canonicalize() {
        Ok(abs_path) => println!("📂 Data directory: {}\n", abs_path.display()),
        Err(_) => println!(
            "📂 Data directory: {} (will be created)\n",
            config.core.dir_path.display()
        ),
    }

    let engine = match LsmEngine::new(config) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("❌ Error initializing LSM Engine: {}", e);
            eprintln!("💡 Tip: if you don't need to recover unflushed writes, rename/delete wal.log and try again.");
            return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string()));
        }
    };

    println!("✓ Engine initialized successfully!");
    println!("🚀 Starting server at {}:{}\n", host, port);

    lsm_kv_store::api::start_server(engine, &host, port).await
}
