use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub max_json_payload_size: usize,
    pub max_raw_payload_size: usize,
    pub feature_cache_ttl_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            max_json_payload_size: 50 * 1024 * 1024,  // 50MB
            max_raw_payload_size: 50 * 1024 * 1024,   // 50MB
            feature_cache_ttl_secs: 10,
        }
    }
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        
        let port = env::var("PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse::<u16>()
            .unwrap_or(8080);

        let max_json_payload_size = env::var("MAX_JSON_PAYLOAD_SIZE")
            .unwrap_or_else(|_| (50 * 1024 * 1024).to_string())
            .parse::<usize>()
            .unwrap_or(50 * 1024 * 1024);

        let max_raw_payload_size = env::var("MAX_RAW_PAYLOAD_SIZE")
            .unwrap_or_else(|_| (50 * 1024 * 1024).to_string())
            .parse::<usize>()
            .unwrap_or(50 * 1024 * 1024);

        let feature_cache_ttl_secs = env::var("FEATURE_CACHE_TTL")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<u64>()
            .unwrap_or(10);

        Self {
            host,
            port,
            max_json_payload_size,
            max_raw_payload_size,
            feature_cache_ttl_secs,
        }
    }

    pub fn print_info(&self) {
        println!("📋 Server Configuration:");
        println!("   Host: {}", self.host);
        println!("   Port: {}", self.port);
        println!("   JSON Payload Limit: {} MB", self.max_json_payload_size / 1024 / 1024);
        println!("   Raw Payload Limit: {} MB", self.max_raw_payload_size / 1024 / 1024);
        println!("   Feature Cache TTL: {}s", self.feature_cache_ttl_secs);
        println!();
    }
}
