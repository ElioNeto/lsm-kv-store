use lsm_kv_store::{LsmConfig, LsmEngine};
use std::env;
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
    println!("║         LSM-Tree REST API Server                      ║");
    println!("╚═══════════════════════════════════════════════════════╝\n");

    // LER VARIÁVEIS DE AMBIENTE
    let data_dir = env::var("DATA_DIR").unwrap_or_else(|_| "./.lsm_data".to_string());

    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);

    // Configurar engine com estrutura modular
    let config = LsmConfig::builder()
        .dir_path(PathBuf::from(data_dir))
        .memtable_max_size(4 * 1024 * 1024) // 4MB
        .build();

    // Mostrar caminho absoluto do diretório de dados
    match config.core.dir_path.canonicalize() {
        Ok(abs_path) => println!("📂 Diretório de dados: {}\n", abs_path.display()),
        Err(_) => println!(
            "📂 Diretório de dados: {} (será criado)\n",
            config.core.dir_path.display()
        ),
    }

    // Inicializar engine
    let engine = match LsmEngine::new(config) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("❌ Erro ao inicializar LSM Engine: {}", e);
            eprintln!("💡 Dica: se você não precisa recuperar writes não-flushados, renomeie/apague o wal.log e tente novamente.");
            return Err(io::Error::new(io::ErrorKind::InvalidData, e.to_string()));
        }
    };

    println!("✓ Engine inicializado com sucesso!");
    println!("🚀 Iniciando servidor em {}:{}\n", host, port);

    // Iniciar servidor HTTP
    lsm_kv_store::api::start_server(engine, &host, port).await
}
