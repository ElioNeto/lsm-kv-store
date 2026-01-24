use lsm_kv_store::{LsmConfig, LsmEngine};
use std::io;
use std::path::PathBuf;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Configurar tracing
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    println!("╔═══════════════════════════════════════════════════════╗");
    println!("║        LSM-Tree REST API Server                       ║");
    println!("╚═══════════════════════════════════════════════════════╝\n");

    // Configurar engine
    let config = LsmConfig {
        memtable_max_size: 4 * 1024 * 1024, // 4MB
        data_dir: PathBuf::from("./.lsm_data"),
    };

    // Mostrar caminho absoluto do diretório de dados
    match config.data_dir.canonicalize() {
        Ok(abs_path) => println!("📂 Diretório de dados: {}\n", abs_path.display()),
        Err(_) => println!(
            "📂 Diretório de dados: {} (será criado)\n",
            config.data_dir.display()
        ),
    }

    //let engine = LsmEngine::new(config).expect("Failed to initialize LSM Engine");
    let engine = match LsmEngine::new(config) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("Erro ao inicializar LSM Engine: {e}");
            eprintln!("Dica: se você não precisa recuperar writes não-flushados, renomeie/apague o wal.log e tente novamente.");
            return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string()));
        }
    };

    // Iniciar servidor HTTP
    lsm_kv_store::api::start_server(engine, "127.0.0.1", 8080).await
}
