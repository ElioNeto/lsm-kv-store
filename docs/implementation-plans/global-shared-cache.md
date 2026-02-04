# Plano de Implementação: Global Shared Block Cache

**Issue:** #35  
**Branch:** `feature/global-shared-cache`  
**Prioridade:** HIGH  
**Estimativa:** 1-2 dias  

---

## 📋 Objetivo

Implementar um cache de blocos global compartilhado entre todas as instâncias de `SstableReader`, reduzindo o consumo de memória de `O(num_sstables * cache_size)` para `O(cache_size)`.

## 🎯 Problema Atual

Cada `SstableReader` possui seu próprio `LruCache<u64, Vec<u8>>` de tamanho configurado (ex: 64MB). Com múltiplas SSTables abertas:

```
100 SSTables × 64MB = 6.4GB de memória
```

Isso desperdiça memória e não respeita o limite global de cache configurado.

## ✅ Solução Proposta

Criar um cache global único compartilhado via `Arc<Mutex<...>>` que armazena blocos de todas as SSTables.

### Arquitetura

```
┌─────────────────────────────────────────┐
│          LsmEngine                      │
│  ┌───────────────────────────────────┐  │
│  │   GlobalBlockCache (Arc)          │  │
│  │   LruCache<CacheKey, Arc<Vec>>    │  │
│  └───────────────────────────────────┘  │
│           ▲           ▲           ▲     │
│           │           │           │     │
│  ┌────────┴────┐ ┌────┴────┐ ┌───┴────┐│
│  │SSTableReader│ │SSTable  │ │SSTable ││
│  │   (Arc)     │ │Reader   │ │Reader  ││
│  └─────────────┘ └─────────┘ └────────┘│
└─────────────────────────────────────────┘
```

---

## 📐 Design Detalhado

### 1. Estrutura `CacheKey`

**Problema:** Chave atual é apenas `u64` (offset do bloco). Com múltiplos arquivos, colisões são inevitáveis.

**Solução:** Chave composta identificando arquivo + offset.

```rust
// src/storage/cache.rs
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::collections::hash_map::DefaultHasher;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    file_id: u64,      // Hash do PathBuf
    block_offset: u64, // Offset do bloco no arquivo
}

impl CacheKey {
    pub fn new(path: &PathBuf, offset: u64) -> Self {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let file_id = hasher.finish();
        
        Self {
            file_id,
            block_offset: offset,
        }
    }
}
```

**Alternativa considerada:** Usar `PathBuf` diretamente como chave.
- ❌ Overhead de memória (paths podem ser longos)
- ❌ Comparação mais lenta
- ✅ Usar hash é mais eficiente

### 2. Estrutura `GlobalBlockCache`

```rust
// src/storage/cache.rs
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

pub struct GlobalBlockCache {
    cache: Mutex<LruCache<CacheKey, Arc<Vec<u8>>>>,
}

impl GlobalBlockCache {
    pub fn new(capacity_mb: usize, block_size: usize) -> Arc<Self> {
        let capacity_bytes = capacity_mb * 1024 * 1024;
        let num_blocks = (capacity_bytes / block_size).max(1);
        let capacity = NonZeroUsize::new(num_blocks).unwrap();
        
        Arc::new(Self {
            cache: Mutex::new(LruCache::new(capacity)),
        })
    }
    
    pub fn get(&self, key: &CacheKey) -> Option<Arc<Vec<u8>>> {
        let mut cache = self.cache.lock().unwrap();
        cache.get(key).cloned()
    }
    
    pub fn put(&self, key: CacheKey, value: Vec<u8>) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(key, Arc::new(value));
    }
    
    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }
    
    // Método para estatísticas (opcional)
    pub fn stats(&self) -> CacheStats {
        let cache = self.cache.lock().unwrap();
        CacheStats {
            len: cache.len(),
            cap: cache.cap().get(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub len: usize,
    pub cap: usize,
}
```

**Decisão de Design: `Arc<Vec<u8>>` em vez de `Vec<u8>`**
- ✅ Evita clonagem de dados ao retornar do cache
- ✅ Permite múltiplas referências ao mesmo bloco
- ✅ Cache hit fica O(1) sem cópia

### 3. Refatoração do `SstableReader`

**Antes:**
```rust
pub struct SstableReader {
    block_cache: LruCache<u64, Vec<u8>>,  // Cache próprio
    // ...
}
```

**Depois:**
```rust
// src/storage/reader.rs
use crate::storage::cache::{GlobalBlockCache, CacheKey};

pub struct SstableReader {
    metadata: MetaBlock,
    bloom_filter: Bloom<[u8]>,
    file: File,
    block_cache: Arc<GlobalBlockCache>,  // ✅ Cache compartilhado
    path: PathBuf,
    config: StorageConfig,
}

impl SstableReader {
    pub fn open(
        path: PathBuf,
        config: StorageConfig,
        block_cache: Arc<GlobalBlockCache>,  // ✅ Injeção de dependência
    ) -> Result<Self> {
        // ... código existente de leitura do arquivo ...
        
        Ok(Self {
            metadata,
            bloom_filter,
            file,
            block_cache,  // Usa o cache compartilhado
            path,
            config,
        })
    }
    
    fn read_block(&mut self, block_meta: &BlockMeta) -> Result<Vec<u8>> {
        let cache_key = CacheKey::new(&self.path, block_meta.offset);
        
        // Tentar obter do cache
        if let Some(cached) = self.block_cache.get(&cache_key) {
            return Ok((*cached).clone());  // Arc -> Vec clone
        }
        
        // Cache miss - ler do disco
        let block_data = self.read_and_decompress_block(block_meta)?;
        
        // Armazenar no cache global
        self.block_cache.put(cache_key, block_data.clone());
        
        Ok(block_data)
    }
}
```

### 4. Atualização do `LsmEngine`

```rust
// src/core/engine.rs
use crate::storage::cache::GlobalBlockCache;

pub struct LsmEngine {
    pub(crate) memtable: Mutex<MemTable>,
    pub(crate) wal: WriteAheadLog,
    pub(crate) sstables: Mutex<Vec<SstableReader>>,
    pub(crate) block_cache: Arc<GlobalBlockCache>,  // ✅ Novo campo
    pub(crate) dir_path: PathBuf,
    pub(crate) config: LsmConfig,
}

impl LsmEngine {
    pub fn new(config: LsmConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.core.dir_path)?;
        
        // ✅ Criar cache global único
        let block_cache = GlobalBlockCache::new(
            config.storage.block_cache_size_mb,
            config.storage.block_size,
        );
        
        let wal = WriteAheadLog::new(&config.core.dir_path)?;
        let wal_records = wal.recover()?;
        
        let mut sstables = Vec::new();
        for entry in std::fs::read_dir(&config.core.dir_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "sst") {
                // ✅ Passar cache para cada reader
                match SstableReader::open(
                    path.clone(),
                    config.storage.clone(),
                    Arc::clone(&block_cache),  // Compartilhar cache
                ) {
                    Ok(sst) => sstables.push(sst),
                    Err(e) => warn!("Failed to load SSTable {}: {}", path.display(), e),
                }
            }
        }
        
        // ... resto do código ...
        
        Ok(Self {
            memtable: Mutex::new(memtable),
            wal,
            sstables: Mutex::new(sstables),
            block_cache,  // ✅ Armazenar referência
            dir_path: config.core.dir_path.clone(),
            config,
        })
    }
    
    fn flush(&self) -> Result<()> {
        // ... código de flush ...
        
        // ✅ Passar cache ao abrir novo SSTable
        let reader = SstableReader::open(
            sst_path,
            self.config.storage.clone(),
            Arc::clone(&self.block_cache),
        )?;
        
        // ...
    }
}
```

---

## 🔧 Ordem de Implementação

### **Fase 1: Estrutura Base** (2-3 horas)

1. ✅ Criar arquivo `src/storage/cache.rs`
2. ✅ Implementar `CacheKey` com testes unitários
3. ✅ Implementar `GlobalBlockCache` com testes unitários
4. ✅ Adicionar `pub mod cache;` em `src/storage/mod.rs`

**Testes:**
```rust
#[test]
fn test_cache_key_uniqueness() {
    let path1 = PathBuf::from("/data/sst1.sst");
    let path2 = PathBuf::from("/data/sst2.sst");
    
    let key1 = CacheKey::new(&path1, 0);
    let key2 = CacheKey::new(&path2, 0);
    
    assert_ne!(key1, key2);  // Diferentes arquivos
}

#[test]
fn test_cache_key_same_file() {
    let path = PathBuf::from("/data/sst1.sst");
    
    let key1 = CacheKey::new(&path, 0);
    let key2 = CacheKey::new(&path, 4096);
    
    assert_ne!(key1, key2);  // Diferentes offsets
    assert_eq!(key1.file_id, key2.file_id);  // Mesmo arquivo
}

#[test]
fn test_global_cache_basic() {
    let cache = GlobalBlockCache::new(1, 4096);  // 1MB, blocos de 4KB
    
    let key = CacheKey::new(&PathBuf::from("test.sst"), 0);
    let data = vec![1, 2, 3, 4];
    
    cache.put(key.clone(), data.clone());
    
    let retrieved = cache.get(&key).unwrap();
    assert_eq!(*retrieved, data);
}
```

### **Fase 2: Refatoração do Reader** (2-3 horas)

1. ✅ Adicionar campo `Arc<GlobalBlockCache>` em `SstableReader`
2. ✅ Atualizar assinatura de `SstableReader::open()`
3. ✅ Refatorar `read_block()` para usar `CacheKey`
4. ✅ Remover campo antigo `block_cache: LruCache<...>`
5. ✅ Atualizar método `calculate_cache_capacity()` (não é mais necessário)

### **Fase 3: Integração na Engine** (1-2 horas)

1. ✅ Adicionar campo `block_cache` em `LsmEngine`
2. ✅ Criar cache em `LsmEngine::new()`
3. ✅ Passar cache para todos os `SstableReader::open()`
4. ✅ Atualizar método `flush()` para passar cache

### **Fase 4: Testes de Integração** (2-3 horas)

```rust
#[test]
fn test_shared_cache_across_sstables() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let cache = GlobalBlockCache::new(1, 4096);
    
    // Criar múltiplas SSTables
    let sst1 = create_test_sstable(dir.path().join("1.sst"), &config, &cache);
    let sst2 = create_test_sstable(dir.path().join("2.sst"), &config, &cache);
    
    // Verificar que ambas usam o mesmo cache
    let stats_before = cache.stats();
    
    sst1.get("key1").unwrap();  // Popula cache
    let stats_after1 = cache.stats();
    assert_eq!(stats_after1.len, stats_before.len + 1);
    
    sst2.get("key2").unwrap();  // Popula cache
    let stats_after2 = cache.stats();
    assert_eq!(stats_after2.len, stats_after1.len + 1);
}

#[test]
fn test_memory_limit_respected() {
    // Criar engine com cache de 1MB
    let config = LsmConfig {
        storage: StorageConfig {
            block_cache_size_mb: 1,
            block_size: 4096,
            // ...
        },
        // ...
    };
    
    let engine = LsmEngine::new(config).unwrap();
    
    // Criar muitas SSTables
    for i in 0..100 {
        insert_and_flush(&engine, i);
    }
    
    let stats = engine.block_cache.stats();
    let max_blocks = (1 * 1024 * 1024) / 4096;
    
    // Cache não deve exceder limite
    assert!(stats.len <= max_blocks);
}
```

### **Fase 5: Benchmarks** (1 hora)

```rust
// benches/cache_benchmark.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_cache_hit(c: &mut Criterion) {
    let cache = GlobalBlockCache::new(64, 4096);
    let key = CacheKey::new(&PathBuf::from("test.sst"), 0);
    cache.put(key.clone(), vec![0u8; 4096]);
    
    c.bench_function("cache_hit", |b| {
        b.iter(|| {
            black_box(cache.get(&key));
        });
    });
}

fn bench_cache_miss(c: &mut Criterion) {
    let cache = GlobalBlockCache::new(64, 4096);
    
    c.bench_function("cache_miss", |b| {
        b.iter(|| {
            let key = CacheKey::new(&PathBuf::from("test.sst"), rand::random());
            black_box(cache.get(&key));
        });
    });
}
```

---

## ⚠️ Considerações de Segurança e Performance

### 1. Contenção de Lock

**Problema:** `Mutex` pode criar gargalo em workloads com muitos cache hits.

**Mitigação:** Por enquanto, usar `Mutex` simples. Em otimização futura:
- Considerar `parking_lot::Mutex` (mais rápido)
- Implementar cache sharded (dividir em N sub-caches)

### 2. Eviction Policy

**Comportamento:** LRU é justo entre arquivos (não privilegia nenhum arquivo específico).

**Validação:** Adicionar teste para garantir eviction balanceada.

### 3. Clonagem de Vec

**Overhead:** `read_block()` retorna `Vec<u8>`, então clonamos o `Arc<Vec<u8>>`.

**Alternativa futura:** Retornar `Arc<Vec<u8>>` diretamente (breaking change na API).

---

## 📊 Métricas de Sucesso

### Critérios de Aceitação

- ✅ Cache único compartilhado entre todas as SSTables
- ✅ Uso de memória = `O(cache_size_mb)` independente do número de arquivos
- ✅ Todos os testes unitários e de integração passando
- ✅ Benchmarks mostram overhead < 5% vs cache individual
- ✅ `cargo clippy` sem warnings

### Métricas de Memória

**Antes:**
```
10 SSTables × 64MB = 640MB
100 SSTables × 64MB = 6.4GB
```

**Depois:**
```
10 SSTables → 64MB total
100 SSTables → 64MB total
```

**Redução:** 10x para 10 arquivos, 100x para 100 arquivos

---

## 🔄 Compatibilidade

### Breaking Changes

✅ **Sim** - A assinatura de `SstableReader::open()` muda:

```rust
// Antes
SstableReader::open(path, config)

// Depois
SstableReader::open(path, config, cache)
```

### Migração

Todos os callers de `SstableReader::open()` precisam ser atualizados:
- `LsmEngine::new()`
- `LsmEngine::flush()`
- Testes em `src/storage/reader.rs`

---

## 📚 Referências

- [LRU Cache in Rust](https://docs.rs/lru/latest/lru/)
- [Arc vs Rc](https://doc.rust-lang.org/std/sync/struct.Arc.html)
- [RocksDB Block Cache](https://github.com/facebook/rocksdb/wiki/Block-Cache)

---

## ✅ Checklist Final

- [ ] Fase 1: Estrutura base implementada
- [ ] Fase 2: Reader refatorado
- [ ] Fase 3: Engine integrada
- [ ] Fase 4: Testes passando
- [ ] Fase 5: Benchmarks executados
- [ ] Documentação atualizada
- [ ] Code review interno
- [ ] PR criado contra `main`
- [ ] Issue #35 fechada
