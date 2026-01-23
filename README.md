# LSM KV Store (Rust)

Key-Value Store local em Rust baseado em **LSM-Tree (Log-Structured Merge-Tree)**, otimizado para alta taxa de escrita e integridade/durabilidade via WAL (Write-Ahead Log).  
Este repositório cobre a **Fase 1**: Storage Engine local (MemTable + WAL + SSTables + Bloom Filter), com foco em clareza, segurança de memória (Rust safe) e evolução incremental.

---

## Visão geral

### Por que LSM-Tree?

A arquitetura LSM-Tree favorece escritas sequenciais (append-only) e organiza dados em estruturas em memória e em disco que são “fundidas” ao longo do tempo (compaction), sendo um padrão amplamente usado em bancos de dados de alto throughput.

### Componentes implementados (Fase 1)

- **MemTable (BTreeMap)**: mantém chaves **em ordem alfabética** na memória (requisito funcional crítico).
- **WAL (Write-Ahead Log)**: toda escrita é persistida no `wal.log` antes de entrar na MemTable, com sincronização síncrona para durabilidade.
- **SSTables imutáveis**: flush da MemTable para arquivos `.sst` ordenados.
- **Bloom Filter por SSTable**: reduz leituras desnecessárias ao pular tabelas onde a chave certamente não existe.
- **Tombstones**: deleções lógicas (`is_deleted`) preparadas para limpeza futura via compaction.

### Roadmap (próximas fases)

- **Compaction Size-Tiered** (merge de SSTables e remoção de tombstones/versões antigas).
- Índices/offsets para busca mais eficiente dentro de SSTable (evitar varredura linear).
- Scripting (Lua/Python) e interfaces TCP/REST (fora do escopo desta fase).

---

## Estrutura do projeto

- `src/lib.rs`: implementação do motor (MemTable, WAL, SSTable, Engine, testes).
- Diretório de dados:
  - Padrão: `./.lsm_data`
  - Arquivos:
    - `wal.log` (append-only)
    - `*.sst` (SSTables imutáveis)

---

## Começando

### Pré-requisitos

- Rust (Edition 2021+) instalado via `rustup`.
- Git.

> Dica: para ver a versão do Rust/Cargo após instalar:  
> `rustc --version`  
> `cargo --version`

### Clonar o repositório

```bash
git clone https://github.com/ElioNeto/lsm-kv-store.git
cd lsm-kv-store
```

````

### Fazer fork (workflow recomendado para contribuir)

1. Clique em **Fork** no GitHub.
2. Clone o seu fork:
   ```bash
   git clone https://github.com/<seu-usuario>/lsm-kv-store.git
   cd lsm-kv-store
   ```
3. Adicione o upstream:
   ```bash
   git remote add upstream https://github.com/ElioNeto/lsm-kv-store.git
   git fetch upstream
   ```

---

## Ambiente de desenvolvimento

### Instalação do Rust (via rustup)

- Acesse: https://rustup.rs e siga as instruções para seu sistema operacional.

### Tooling recomendado

Instalar ferramentas padrão de qualidade:

```bash
rustup component add rustfmt clippy
```

Formatar código:

```bash
cargo fmt
```

Rodar lints:

```bash
cargo clippy --all-targets --all-features -D warnings
```

---

## Build, testes e execução

### Baixar dependências e compilar

```bash
cargo build
```

### Rodar a suíte de testes

```bash
cargo test
```

### Executar “o projeto”

No momento, este repositório é principalmente uma **biblioteca** (storage engine).
A forma mais direta de executar e validar é via testes (`cargo test`) e/ou um exemplo.

#### Executar por exemplo (recomendado)

Crie um arquivo `examples/basic.rs` com:

```rust
use lsm_kv_store::{LsmConfig, LsmEngine};
use tempfile::tempdir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let config = LsmConfig {
        memtable_max_size: 4 * 1024,
        data_dir: dir.path().to_path_buf(),
    };

    let engine = LsmEngine::new(config)?;
    engine.set("hello".to_string(), b"world".to_vec())?;

    let v = engine.get("hello")?;
    println!("GET hello = {:?}", v.map(|x| String::from_utf8_lossy(&x).to_string()));

    Ok(())
}
```

Depois rode:

```bash
cargo run --example basic
```

### Benchmarks (opcional)

Se você adicionar benchmarks (há configuração para Criterion no `Cargo.toml`):

```bash
cargo bench
```

---

## Detalhes técnicos (Fase 1)

### Modelo de dados

`LogRecord` é serializado em binário e inclui:

- `key: String`
- `value: Vec<u8>`
- `timestamp: u128` (nanosegundos)
- `is_deleted: bool` (tombstone)

### Fluxo de escrita (SET/DELETE)

1. Serializa registro.
2. Anexa no WAL (`wal.log`) e sincroniza em disco.
3. Insere na MemTable (BTreeMap).
4. Ao atingir o limite configurado, faz flush para SSTable e reinicia o WAL.

### Fluxo de leitura (GET)

1. Consulta MemTable.
2. Se não encontrar, consulta SSTables do mais recente para o mais antigo.
3. Antes de ler uma SSTable, consulta o Bloom Filter para evitar I/O em chaves inexistentes.

---

## Contribuição

PRs são bem-vindos, especialmente para:

- Compaction (Size-Tiered) e remoção de tombstones.
- Índices/offsets para leitura mais eficiente de SSTables.
- Testes de falha (simulação de crash/recovery) e validação de integridade.
- Refinar formato de arquivo (metadados, checksum, versionamento).

Sugestão de fluxo:

- Crie uma branch: `git checkout -b feat/minha-feature`
- Commits pequenos e descritivos
- Abra PR com descrição clara do objetivo e dos testes executados


````
