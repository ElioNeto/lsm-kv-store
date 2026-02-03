use std::mem::size_of;

/// Tamanho padrão de um bloco (4KB)
pub const BLOCK_SIZE: usize = 4096;

/// Tamanho de um u16 em bytes (usado para key_len, val_len e offsets)
const U16_SIZE: usize = size_of::<u16>();

/// Block é a unidade atômica de armazenamento para SSTables.
/// Agrupa múltiplos pares key-value em um bloco de tamanho fixo,
/// permitindo cache eficiente e seeking rápido.
#[derive(Debug, Clone)]
pub struct Block {
    /// Dados brutos do bloco (key-value pairs serializados)
    data: Vec<u8>,
    /// Offsets para o início de cada record dentro do bloco
    offsets: Vec<u16>,
    /// Tamanho máximo do bloco em bytes
    block_size: usize,
}

impl Block {
    /// Cria um novo bloco vazio com o tamanho especificado
    pub fn new(block_size: usize) -> Self {
        Self {
            data: Vec::new(),
            offsets: Vec::new(),
            block_size,
        }
    }

    /// Calcula o espaço necessário para adicionar um par key-value
    /// Formato: [key_len: u16][key_bytes][val_len: u16][val_bytes]
    fn entry_size(key: &[u8], value: &[u8]) -> usize {
        U16_SIZE + key.len() + U16_SIZE + value.len()
    }

    /// Calcula o overhead de metadados (offsets + contador)
    /// Layout final: [Records...][Offsets...][num_of_elements: u16]
    fn metadata_size(num_entries: usize) -> usize {
        (num_entries * U16_SIZE) + U16_SIZE
    }

    /// Retorna o espaço utilizado atualmente (dados + metadados)
    fn current_size(&self) -> usize {
        self.data.len() + Self::metadata_size(self.offsets.len())
    }

    /// Tenta adicionar um par key-value ao bloco.
    ///
    /// # Retorno
    /// - `true`: par adicionado com sucesso
    /// - `false`: não há espaço suficiente no bloco
    ///
    /// # Formato de serialização por record
    /// ```text
    /// [key_len: u16][key_bytes][val_len: u16][val_bytes]
    /// ```
    pub fn add(&mut self, key: &[u8], value: &[u8]) -> bool {
        let entry_size = Self::entry_size(key, value);
        let new_offset_size = U16_SIZE; // adicionar um novo offset
        let total_needed = self.current_size() + entry_size + new_offset_size;

        // Verifica se há espaço suficiente
        if total_needed > self.block_size {
            return false;
        }

        // Registra o offset do início deste record
        let offset = self.data.len() as u16;
        self.offsets.push(offset);

        // Serializa: [key_len: u16][key_bytes][val_len: u16][val_bytes]
        let key_len = key.len() as u16;
        let val_len = value.len() as u16;

        self.data.extend_from_slice(&key_len.to_le_bytes());
        self.data.extend_from_slice(key);
        self.data.extend_from_slice(&val_len.to_le_bytes());
        self.data.extend_from_slice(value);

        true
    }

    /// Serializa o bloco para gravação em disco.
    ///
    /// # Layout do bloco serializado
    /// ```text
    /// [Record 1]...[Record N][Offset 1: u16]...[Offset N: u16][num_of_elements: u16]
    /// ```
    ///
    /// O `num_of_elements` no final permite encontrar onde os offsets começam
    /// durante a decodificação, calculando: `data.len() - 2 - (num_of_elements * 2)`
    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(self.current_size());

        // 1. Escrever todos os records
        encoded.extend_from_slice(&self.data);

        // 2. Escrever lista de offsets
        for &offset in &self.offsets {
            encoded.extend_from_slice(&offset.to_le_bytes());
        }

        // 3. Escrever número de elementos (para facilitar decode)
        let num_elements = self.offsets.len() as u16;
        encoded.extend_from_slice(&num_elements.to_le_bytes());

        encoded
    }

    /// Desserializa um bloco a partir de bytes.
    ///
    /// # Layout esperado
    /// ```text
    /// [Records...][Offsets...][num_of_elements: u16]
    /// ```
    pub fn decode(data: &[u8]) -> Self {
        if data.len() < U16_SIZE {
            return Self {
                data: Vec::new(),
                offsets: Vec::new(),
                block_size: BLOCK_SIZE,
            };
        }

        // 1. Ler num_of_elements (últimos 2 bytes)
        let num_elements_start = data.len() - U16_SIZE;
        let num_elements =
            u16::from_le_bytes([data[num_elements_start], data[num_elements_start + 1]]) as usize;

        // 2. Calcular onde começam os offsets
        let offsets_start = data.len() - U16_SIZE - (num_elements * U16_SIZE);

        // 3. Extrair records (tudo antes dos offsets)
        let records_data = data[..offsets_start].to_vec();

        // 4. Extrair offsets
        let mut offsets = Vec::with_capacity(num_elements);
        let mut offset_pos = offsets_start;
        for _ in 0..num_elements {
            let offset = u16::from_le_bytes([data[offset_pos], data[offset_pos + 1]]);
            offsets.push(offset);
            offset_pos += U16_SIZE;
        }

        Self {
            data: records_data,
            offsets,
            block_size: BLOCK_SIZE,
        }
    }

    /// Retorna o número de entries no bloco
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    /// Verifica se o bloco está vazio
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Retorna o tamanho dos dados (sem metadados)
    pub fn data_size(&self) -> usize {
        self.data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_block_is_empty() {
        let block = Block::new(BLOCK_SIZE);
        assert_eq!(block.len(), 0);
        assert!(block.is_empty());
        assert_eq!(block.data_size(), 0);
    }

    #[test]
    fn test_add_single_entry() {
        let mut block = Block::new(BLOCK_SIZE);
        let key = b"test_key";
        let value = b"test_value";

        let success = block.add(key, value);
        assert!(success, "Should successfully add entry");
        assert_eq!(block.len(), 1);
        assert!(!block.is_empty());
    }

    #[test]
    fn test_add_multiple_entries() {
        let mut block = Block::new(BLOCK_SIZE);

        for i in 0..10 {
            let key = format!("key_{:03}", i);
            let value = format!("value_{:03}", i);
            let success = block.add(key.as_bytes(), value.as_bytes());
            assert!(success, "Should add entry {}", i);
        }

        assert_eq!(block.len(), 10);
    }

    #[test]
    fn test_add_until_full() {
        let mut block = Block::new(256); // Bloco pequeno para testar limite
        let mut added_count = 0;

        // Tentar adicionar muitas entries até o bloco ficar cheio
        for i in 0..100 {
            let key = format!("k{}", i);
            let value = format!("v{}", i);
            if block.add(key.as_bytes(), value.as_bytes()) {
                added_count += 1;
            } else {
                break; // Bloco cheio
            }
        }

        assert!(added_count > 0, "Should have added at least one entry");
        assert!(
            added_count < 100,
            "Should not have added all entries (block is full)"
        );

        // Tentar adicionar mais uma vez deve falhar
        let result = block.add(b"extra_key", b"extra_value");
        assert!(!result, "Should reject entry when block is full");
    }

    #[test]
    fn test_overflow_large_entry() {
        let mut block = Block::new(128); // Bloco muito pequeno

        // Criar uma entry que sozinha é maior que o bloco
        let large_key = vec![b'x'; 100];
        let large_value = vec![b'y'; 100];

        let result = block.add(&large_key, &large_value);
        assert!(!result, "Should reject oversized entry");
        assert_eq!(block.len(), 0, "Block should remain empty");
    }

    #[test]
    fn test_encode_decode_empty_block() {
        let block = Block::new(BLOCK_SIZE);
        let encoded = block.encode();
        let decoded = Block::decode(&encoded);

        assert_eq!(decoded.len(), 0);
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_encode_decode_single_entry() {
        let mut block = Block::new(BLOCK_SIZE);
        block.add(b"key1", b"value1");

        let encoded = block.encode();
        let decoded = Block::decode(&encoded);

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded.data_size(), block.data_size());
        assert_eq!(decoded.offsets, block.offsets);
        assert_eq!(decoded.data, block.data);
    }

    #[test]
    fn test_encode_decode_multiple_entries() {
        let mut block = Block::new(BLOCK_SIZE);

        let entries = vec![
            (b"apple" as &[u8], b"red" as &[u8]),
            (b"banana", b"yellow"),
            (b"cherry", b"red"),
            (b"date", b"brown"),
            (b"elderberry", b"purple"),
        ];

        for (key, value) in &entries {
            assert!(block.add(key, value));
        }

        let encoded = block.encode();
        let decoded = Block::decode(&encoded);

        assert_eq!(decoded.len(), entries.len());
        assert_eq!(decoded.data, block.data);
        assert_eq!(decoded.offsets, block.offsets);
    }

    #[test]
    fn test_encode_decode_round_trip() {
        let mut block = Block::new(BLOCK_SIZE);

        // Adicionar várias entries com tamanhos variados
        for i in 0..20 {
            let key = format!("key_{:04}", i);
            let value = format!("value_{}_{}", i, "x".repeat(i % 10));
            block.add(key.as_bytes(), value.as_bytes());
        }

        let original_len = block.len();
        let original_data_size = block.data_size();

        // Round-trip: encode -> decode
        let encoded = block.encode();
        let decoded = Block::decode(&encoded);

        // Verificar integridade
        assert_eq!(decoded.len(), original_len);
        assert_eq!(decoded.data_size(), original_data_size);
        assert_eq!(decoded.data, block.data);
        assert_eq!(decoded.offsets, block.offsets);
    }

    #[test]
    fn test_block_size_calculation() {
        let mut block = Block::new(BLOCK_SIZE);

        let key = b"test";
        let value = b"data";

        let size_before = block.current_size();
        block.add(key, value);
        let size_after = block.current_size();

        // Verificar que o tamanho aumentou corretamente
        let expected_increase = Block::entry_size(key, value) + U16_SIZE; // entry + 1 offset
        assert_eq!(size_after - size_before, expected_increase);
    }

    #[test]
    fn test_decode_malformed_empty_data() {
        let decoded = Block::decode(&[]);
        assert_eq!(decoded.len(), 0);
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_decode_malformed_single_byte() {
        let decoded = Block::decode(&[0xFF]);
        assert_eq!(decoded.len(), 0);
    }

    #[test]
    fn test_large_block_utilization() {
        let mut block = Block::new(BLOCK_SIZE);
        let mut count = 0;

        // Preencher o bloco com entries de tamanho médio
        loop {
            let key = format!("key_{:06}", count);
            let value = format!("value_{:06}_{}", count, "data".repeat(5));
            if !block.add(key.as_bytes(), value.as_bytes()) {
                break;
            }
            count += 1;
        }

        assert!(count > 10, "Should fit many entries in 4KB block");

        // Verificar que o tamanho não excede o limite
        assert!(block.current_size() <= BLOCK_SIZE);

        // Round-trip test
        let encoded = block.encode();
        let decoded = Block::decode(&encoded);
        assert_eq!(decoded.len(), count);
    }
}
