use lsm_kv_store::{LsmConfig, LsmEngine};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Inicializar tracing para ver logs internos
    tracing_subscriber::fmt::init();

    println!("=== LSM-Tree Key-Value Store Demo ===\n");

    // Configuração personalizada (MemTable pequena para forçar flush)
    let config = LsmConfig {
        memtable_max_size: 200, // 200 bytes (pequeno para demonstração)
        data_dir: PathBuf::from("./demo_data"),
    };

    // Criar/abrir o engine
    println!("1. Inicializando LSM Engine...");
    let db = LsmEngine::new(config)?;
    println!("{}\n", db.stats());

    // === FEATURE 1: SET (Escrita) ===
    println!("2. Testando SET (escrita):");
    db.set("user:1".to_string(), b"Alice".to_vec())?;
    db.set("user:2".to_string(), b"Bob".to_vec())?;
    db.set("user:3".to_string(), b"Charlie".to_vec())?;
    println!("   ✓ Inseridos: user:1, user:2, user:3");
    println!("{}\n", db.stats());

    // === FEATURE 2: GET (Leitura) ===
    println!("3. Testando GET (leitura da MemTable):");
    if let Some(value) = db.get("user:1")? {
        println!("   user:1 = {}", String::from_utf8_lossy(&value));
    }
    if let Some(value) = db.get("user:2")? {
        println!("   user:2 = {}", String::from_utf8_lossy(&value));
    }
    println!();

    // === FEATURE 3: Flush Automático (MemTable → SSTable) ===
    println!("4. Forçando flush com dados grandes:");
    db.set(
        "product:1".to_string(),
        b"Notebook Dell Inspiron 15 - 16GB RAM, 512GB SSD, Intel i7".to_vec(),
    )?;
    println!("   ✓ Flush automático disparado (MemTable atingiu limite)");
    println!("{}\n", db.stats());

    // === FEATURE 4: Leitura de SSTable (dados no disco) ===
    println!("5. Lendo dados que foram para SSTable:");
    if let Some(value) = db.get("user:1")? {
        println!(
            "   user:1 = {} (lido da SSTable)",
            String::from_utf8_lossy(&value)
        );
    }
    println!();

    // === FEATURE 5: UPDATE (sobrescrever chave) ===
    println!("6. Testando UPDATE (sobrescrever valor):");
    db.set("user:1".to_string(), b"Alice Smith".to_vec())?;
    if let Some(value) = db.get("user:1")? {
        println!(
            "   user:1 = {} (valor atualizado)",
            String::from_utf8_lossy(&value)
        );
    }
    println!();

    // === FEATURE 6: DELETE (Tombstone) ===
    println!("7. Testando DELETE (tombstone):");
    db.delete("user:2".to_string())?;
    match db.get("user:2")? {
        Some(_) => println!("   ✗ Erro: user:2 ainda existe"),
        None => println!("   ✓ user:2 deletado com sucesso (tombstone criado)"),
    }
    println!();

    // === FEATURE 7: Busca em chave inexistente (Bloom Filter) ===
    println!("8. Testando busca em chave inexistente:");
    println!("   Buscando 'nonexistent:key' (Bloom Filter deve evitar leitura de disco)");
    match db.get("nonexistent:key")? {
        Some(_) => println!("   ✗ Erro inesperado"),
        None => println!("   ✓ Chave não encontrada (Bloom Filter funcionou)"),
    }
    println!();

    // === FEATURE 8: Múltiplas escritas (forçar mais flushes) ===
    println!("9. Inserindo mais dados para criar múltiplas SSTables:");
    for i in 10..15 {
        db.set(format!("item:{}", i), format!("Value {}", i).into_bytes())?;
    }
    println!("   ✓ Inseridos: item:10 até item:14");
    println!("{}\n", db.stats());

    // === FEATURE 9: Ordem alfabética (BTreeMap) ===
    println!("10. Demonstração de ordenação alfabética:");
    db.set("zebra".to_string(), b"last".to_vec())?;
    db.set("apple".to_string(), b"first".to_vec())?;
    db.set("mango".to_string(), b"middle".to_vec())?;
    println!("   ✓ Inseridos: zebra, apple, mango");
    println!("   (MemTable mantém ordem: apple → mango → zebra)");
    println!("{}\n", db.stats());

    // === FEATURE 10: Persistência (simulação) ===
    println!("11. Simulando reinício do sistema:");
    println!("   Destruindo engine atual e recriando...");
    drop(db);

    let config2 = LsmConfig {
        memtable_max_size: 200,
        data_dir: PathBuf::from("./demo_data"),
    };
    let db2 = LsmEngine::new(config2)?;
    println!("   ✓ Engine recriado (WAL e SSTables recuperados)");
    println!("{}\n", db2.stats());

    // Verificar se dados persistiram
    println!("12. Verificando persistência:");
    if let Some(value) = db2.get("user:1")? {
        println!("   user:1 = {} ✓", String::from_utf8_lossy(&value));
    }
    if let Some(value) = db2.get("apple")? {
        println!("   apple = {} ✓", String::from_utf8_lossy(&value));
    }
    if let Some(value) = db2.get("product:1")? {
        println!("   product:1 = {} ✓", String::from_utf8_lossy(&value));
    }
    println!();

    println!("=== Demo concluída com sucesso! ===");
    println!("\nArquivos criados em: ./demo_data/");
    println!("  - wal.log (Write-Ahead Log)");
    println!("  - *.sst (SSTables imutáveis)");

    Ok(())
}
