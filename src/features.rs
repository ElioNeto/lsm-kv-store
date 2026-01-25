use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::engine::LsmEngine;
use crate::error::Result;

/// Configuração de uma feature flag individual
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureFlag {
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
}

/// Container de todas as feature flags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Features {
    #[serde(default)]
    pub version: u64,
    pub flags: HashMap<String, FeatureFlag>,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            version: 0,
            flags: HashMap::new(),
        }
    }
}

/// Cliente para gerenciar feature flags com cache
pub struct FeatureClient {
    engine: Arc<LsmEngine>,
    cache: Arc<RwLock<Option<(Features, Instant)>>>,
    cache_ttl: Duration,
}

impl FeatureClient {
    const KEY: &'static str = "feature:all";

    pub fn new(engine: Arc<LsmEngine>, cache_ttl: Duration) -> Self {
        Self {
            engine,
            cache: Arc::new(RwLock::new(None)),
            cache_ttl,
        }
    }

    /// Carrega todas as features (com cache)
    fn load_features(&self) -> Result<Features> {
        // Verificar cache
        {
            let cache = self.cache.read().unwrap();
            if let Some((features, timestamp)) = cache.as_ref() {
                if timestamp.elapsed() < self.cache_ttl {
                    return Ok(features.clone());
                }
            }
        }

        // Cache miss ou expirado - carregar do engine
        let bytes = match self.engine.get(Self::KEY)? {
            Some(b) => b,
            None => {
                // Primeira vez - criar estrutura vazia
                let features = Features::default();
                let json = serde_json::to_vec(&features)
                    .map_err(|e| crate::error::LsmError::SerializationFailed(e.to_string()))?;
                self.engine.set(Self::KEY.to_string(), json)?;
                return Ok(features);
            }
        };

        let features: Features = serde_json::from_slice(&bytes)
            .map_err(|e| crate::error::LsmError::DeserializationFailed(e.to_string()))?;

        // Atualizar cache
        let mut cache = self.cache.write().unwrap();
        *cache = Some((features.clone(), Instant::now()));

        Ok(features)
    }

    /// Invalida o cache
    fn invalidate_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        *cache = None;
    }

    /// Verifica se uma feature está habilitada
    pub fn is_enabled(&self, flag_name: &str) -> Result<bool> {
        let features = self.load_features()?;
        Ok(features
            .flags
            .get(flag_name)
            .map(|f| f.enabled)
            .unwrap_or(false))
    }

    /// Lista todas as features
    pub fn list_all(&self) -> Result<Features> {
        self.load_features()
    }

    /// Atualiza uma feature flag específica
    pub fn set_flag(
        &self,
        flag_name: String,
        enabled: bool,
        description: Option<String>,
    ) -> Result<()> {
        // Retry com optimistic locking
        for attempt in 0..5 {
            let mut features = self.load_features()?;

            // Atualizar ou criar flag
            features
                .flags
                .entry(flag_name.clone())
                .and_modify(|f| {
                    f.enabled = enabled;
                    if let Some(desc) = &description {
                        f.description = desc.clone();
                    }
                })
                .or_insert(FeatureFlag {
                    enabled,
                    description: description.clone().unwrap_or_default(),
                });

            features.version += 1;

            // Serializar e salvar
            let json = serde_json::to_vec(&features)
                .map_err(|e| crate::error::LsmError::SerializationFailed(e.to_string()))?;

            match self.engine.set(Self::KEY.to_string(), json) {
                Ok(_) => {
                    self.invalidate_cache();
                    return Ok(());
                }
                Err(_) if attempt < 4 => {
                    // Retry com backoff exponencial
                    std::thread::sleep(Duration::from_millis(10 * 2u64.pow(attempt)));
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Err(crate::error::LsmError::ConcurrentModification)
    }

    /// Remove uma feature flag
    pub fn remove_flag(&self, flag_name: &str) -> Result<bool> {
        let mut features = self.load_features()?;
        let removed = features.flags.remove(flag_name).is_some();

        if removed {
            features.version += 1;
            let json = serde_json::to_vec(&features)
                .map_err(|e| crate::error::LsmError::SerializationFailed(e.to_string()))?;
            self.engine.set(Self::KEY.to_string(), json)?;
            self.invalidate_cache();
        }

        Ok(removed)
    }
}
