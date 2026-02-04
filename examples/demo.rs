use lsm_kv_store::{LsmConfig, LsmEngine, Result};
use tempfile::tempdir;

fn main() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().to_path_buf();

    // Part 1: Create and populate an LSM-tree database
    println!("=== Part 1: Creating LSM-tree database ===");
    let config = LsmConfig::builder()
        .dir_path(path.clone())
        .memtable_max_size(1024)
        .build()?;

    let db = LsmEngine::new(config)?;

    // Insert some key-value pairs
    println!("Inserting keys...");
    db.set("apple".to_string(), b"A red fruit".to_vec())?;
    db.set("banana".to_string(), b"A yellow fruit".to_vec())?;
    db.set("cherry".to_string(), b"A small red fruit".to_vec())?;

    // Read them back
    if let Some(value) = db.get("apple")? {
        println!("apple: {}", String::from_utf8_lossy(&value));
    }

    if let Some(value) = db.get("banana")? {
        println!("banana: {}", String::from_utf8_lossy(&value));
    }

    // Update a key
    println!("\nUpdating 'banana'...");
    db.set("banana".to_string(), b"A VERY yellow fruit".to_vec())?;

    if let Some(value) = db.get("banana")? {
        println!("banana (updated): {}", String::from_utf8_lossy(&value));
    }

    // Delete a key
    println!("\nDeleting 'cherry'...");
    db.delete("cherry".to_string())?;

    match db.get("cherry")? {
        Some(_) => println!("cherry: still exists (unexpected!)"),
        None => println!("cherry: deleted"),
    }

    // Insert more data to trigger automatic flush
    println!("\n=== Part 2: Adding data (automatic flush will occur) ===");
    for i in 0..100 {
        let key = format!("key_{:03}", i);
        let value = format!("value_{}", i);
        db.set(key, value.into_bytes())?;
    }

    println!("Data inserted (memtable will flush automatically when full)");

    // Read some keys
    if let Some(value) = db.get("key_042")? {
        println!("key_042: {}", String::from_utf8_lossy(&value));
    }

    if let Some(value) = db.get("apple")? {
        println!("apple: {}", String::from_utf8_lossy(&value));
    }

    // Part 3: Add more data to create multiple levels
    println!("\n=== Part 3: Adding more data ===");
    for i in 100..200 {
        let key = format!("key_{:03}", i);
        let value = format!("value_{}", i);
        db.set(key, value.into_bytes())?;
    }

    println!("\nDatabase operations complete.");
    println!("Total keys in database: ~200");

    // Part 4: Reopen the database
    println!("\n=== Part 4: Reopening database ===");
    drop(db);

    let config2 = LsmConfig::builder()
        .dir_path(path)
        .memtable_max_size(1024)
        .build()?;

    let db2 = LsmEngine::new(config2)?;

    // Verify data persisted
    if let Some(value) = db2.get("apple")? {
        println!("apple (after reopen): {}", String::from_utf8_lossy(&value));
    }

    if let Some(value) = db2.get("key_042")? {
        println!("key_042 (after reopen): {}", String::from_utf8_lossy(&value));
    }

    if let Some(value) = db2.get("key_150")? {
        println!("key_150 (after reopen): {}", String::from_utf8_lossy(&value));
    }

    println!("\n✅ Demo complete!");
    Ok(())
}
