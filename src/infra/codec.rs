use crate::infra::error::Result; // Import corrigido
use bincode::Options;
use serde::{de::DeserializeOwned, Serialize};

fn opts() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
}

pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(opts().serialize(value)?)
}

pub fn decode<T: DeserializeOwned>(data: &[u8]) -> Result<T> {
    // CORREÇÃO: Especificamos o tipo de fallback para bincode
    Ok(opts().deserialize::<T>(data)?)
}
