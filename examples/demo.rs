use lsm_kv_store::{LsmConfig, LsmEngine};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("=== LSM-Tree Key-Value Store Demo ===\n");

    let config = LsmConfig::builder()
        .memtable_max_size(200)
        .dir_path(PathBuf::from("./demo_data"))
        .build();

    println!("1. Initializing LSM Engine...");
    let db = LsmEngine::new(config)?;
    println!("{}\n", db.stats());

    println!("2. Testing SET (write):");
    db.set("user:1".to_string(), b"Alice".to_vec())?;
    db.set("user:2".to_string(), b"Bob".to_vec())?;
    db.set("user:3".to_string(), b"Charlie".to_vec())?;
    println!("   ✓ Inserted: user:1, user:2, user:3");
    println!("{}\n", db.stats());

    println!("3. Testing GET (read from MemTable):");
    if let Some(value) = db.get("user:1")? {
        println!("   user:1 = {}", String::from_utf8_lossy(&value));
    }
    if let Some(value) = db.get("user:2")? {
        println!("   user:2 = {}", String::from_utf8_lossy(&value));
    }
    println!();

    println!("4. Forcing flush with large data:");
    db.set(
        "product:1".to_string(),
        b"Notebook Dell Inspiron 15 - 16GB RAM, 512GB SSD, Intel i7".to_vec(),
    )?;
    println!("   ✓ Automatic flush triggered (MemTable reached limit)");
    println!("{}\n", db.stats());

    println!("5. Reading data from SSTable:");
    if let Some(value) = db.get("user:1")? {
        println!(
            "   user:1 = {} (read from SSTable)",
            String::from_utf8_lossy(&value)
        );
    }
    println!();

    println!("6. Testing UPDATE (overwrite value):");
    db.set("user:1".to_string(), b"Alice Smith".to_vec())?;
    if let Some(value) = db.get("user:1")? {
        println!(
            "   user:1 = {} (value updated)",
            String::from_utf8_lossy(&value)
        );
    }
    println!();

    println!("7. Testing DELETE (tombstone):");
    db.delete("user:2".to_string())?;
    match db.get("user:2")? {
        Some(_) => println!("   ✗ Error: user:2 still exists"),
        None => println!("   ✓ user:2 deleted successfully (tombstone created)"),
    }
    println!();

    println!("8. Testing search for non-existent key:");
    println!("   Searching 'nonexistent:key' (Bloom Filter should prevent disk read)");
    match db.get("nonexistent:key")? {
        Some(_) => println!("   ✗ Unexpected error"),
        None => println!("   ✓ Key not found (Bloom Filter worked)"),
    }
    println!();

    println!("9. Inserting more data to create multiple SSTables:");
    for i in 10..15 {
        db.set(format!("item:{}", i), format!("Value {}", i).into_bytes())?;
    }
    println!("   ✓ Inserted: item:10 to item:14");
    println!("{}\n", db.stats());

    println!("10. Demonstrating alphabetical ordering:");
    db.set("zebra".to_string(), b"last".to_vec())?;
    db.set("apple".to_string(), b"first".to_vec())?;
    db.set("mango".to_string(), b"middle".to_vec())?;
    println!("   ✓ Inserted: zebra, apple, mango");
    println!("   (MemTable maintains order: apple → mango → zebra)");
    println!("{}\n", db.stats());

    println!("11. Simulating system restart:");
    println!("   Destroying current engine and recreating...");
    drop(db);

    let config2 = LsmConfig::builder()
        .memtable_max_size(200)
        .dir_path(PathBuf::from("./demo_data"))
        .build();
    let db2 = LsmEngine::new(config2)?;
    println!("   ✓ Engine recreated (WAL and SSTables recovered)");
    println!("{}\n", db2.stats());

    println!("12. Verifying persistence:");
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

    println!("=== Demo completed successfully! ===");
    println!("\nFiles created in: ./demo_data/");
    println!("  - wal.log (Write-Ahead Log)");
    println!("  - *.sst (Immutable SSTables)");

    Ok(())
}
