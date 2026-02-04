mod config;

use actix_cors::Cors;
use actix_web::{delete, get, post, web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

use crate::core::engine::LsmEngine;
use crate::features::FeatureClient;

pub use config::ServerConfig;

pub struct AppState {
    pub engine: Arc<LsmEngine>,
    pub features: Arc<FeatureClient>,
}

#[derive(Deserialize)]
pub struct SetRequest {
    pub key: String,
    pub value: String,
}

#[derive(Deserialize)]
pub struct BatchSetRequest {
    pub records: Vec<SetRequest>,
}

#[derive(Deserialize)]
pub struct BatchDeleteRequest {
    pub keys: Vec<String>,
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default)]
    pub prefix: bool,
}

#[derive(Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct SetFeatureRequest {
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
}

#[derive(Serialize)]
pub struct FeatureResponse {
    pub name: String,
    pub enabled: bool,
    pub description: String,
}

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().json(ApiResponse {
        success: true,
        message: "LSM-Tree API is running".to_string(),
        data: None,
    })
}

#[get("/stats")]
async fn get_stats(data: web::Data<AppState>) -> impl Responder {
    let stats = data.engine.stats();
    HttpResponse::Ok().json(ApiResponse {
        success: true,
        message: "Stats retrieved".to_string(),
        data: Some(serde_json::json!({ "stats": stats })),
    })
}

#[get("/stats/all")]
async fn get_stats_all(data: web::Data<AppState>) -> impl Responder {
    match data.engine.stats_all() {
        Ok(stats) => HttpResponse::Ok().json(ApiResponse {
            success: true,
            message: "Stats retrieved".to_string(),
            data: Some(serde_json::to_value(stats).unwrap_or_default()),
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

#[get("/keys/{key}")]
async fn get_key(path: web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    let key = path.into_inner();

    match data.engine.get(&key) {
        Ok(Some(value)) => {
            let value_str = String::from_utf8_lossy(&value).to_string();
            HttpResponse::Ok().json(ApiResponse {
                success: true,
                message: "Key found".to_string(),
                data: Some(serde_json::json!({
                    "key": key,
                    "value": value_str
                })),
            })
        }
        Ok(None) => HttpResponse::NotFound().json(ApiResponse {
            success: false,
            message: format!("Key '{}' not found", key),
            data: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

#[post("/keys")]
async fn set_key(req: web::Json<SetRequest>, data: web::Data<AppState>) -> impl Responder {
    let value_bytes = req.value.as_bytes().to_vec();

    match data.engine.set(req.key.clone(), value_bytes) {
        Ok(_) => HttpResponse::Ok().json(ApiResponse {
            success: true,
            message: format!("Key '{}' set successfully", req.key),
            data: Some(serde_json::json!({ "key": req.key })),
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

#[post("/keys/batch")]
async fn set_batch(req: web::Json<BatchSetRequest>, data: web::Data<AppState>) -> impl Responder {
    let records: Vec<(String, Vec<u8>)> = req
        .records
        .iter()
        .map(|r| (r.key.clone(), r.value.as_bytes().to_vec()))
        .collect();

    match data.engine.set_batch(records) {
        Ok(count) => HttpResponse::Ok().json(ApiResponse {
            success: true,
            message: format!("{} keys inserted successfully", count),
            data: Some(serde_json::json!({ "count": count })),
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

#[delete("/keys/{key}")]
async fn delete_key(path: web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    let key = path.into_inner();

    match data.engine.delete(key.clone()) {
        Ok(_) => HttpResponse::Ok().json(ApiResponse {
            success: true,
            message: format!("Key '{}' deleted successfully", key),
            data: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

#[get("/keys")]
async fn list_keys(data: web::Data<AppState>) -> impl Responder {
    match data.engine.keys() {
        Ok(keys) => {
            let filtered_keys: Vec<String> = keys
                .into_iter()
                .filter(|k: &String| !k.starts_with("feature:"))
                .collect();

            HttpResponse::Ok().json(ApiResponse {
                success: true,
                message: format!("{} keys found", filtered_keys.len()),
                data: Some(serde_json::json!({ "keys": filtered_keys })),
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

#[get("/keys/search")]
async fn search_keys(query: web::Query<SearchQuery>, data: web::Data<AppState>) -> impl Responder {
    let results = if query.prefix {
        data.engine.search_prefix(&query.q)
    } else {
        data.engine.search(&query.q)
    };

    match results {
        Ok(records) => {
            let records_json: Vec<serde_json::Value> = records
                .into_iter()
                .map(|(k, v): (String, Vec<u8>)| {
                    serde_json::json!({
                        "key": k,
                        "value": String::from_utf8_lossy(&v).to_string()
                    })
                })
                .collect();

            HttpResponse::Ok().json(ApiResponse {
                success: true,
                message: format!("{} keys found matching '{}'", records_json.len(), query.q),
                data: Some(serde_json::json!({ "records": records_json })),
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

#[get("/scan")]
async fn scan_all(data: web::Data<AppState>) -> impl Responder {
    match data.engine.scan() {
        Ok(records) => {
            let records_json: Vec<serde_json::Value> = records
                .into_iter()
                .filter(|(k, _): &(String, Vec<u8>)| !k.starts_with("feature:"))
                .map(|(k, v): (String, Vec<u8>)| {
                    serde_json::json!({
                        "key": k,
                        "value": String::from_utf8_lossy(&v).to_string()
                    })
                })
                .collect();

            HttpResponse::Ok().json(ApiResponse {
                success: true,
                message: format!("{} records found", records_json.len()),
                data: Some(serde_json::json!({ "records": records_json })),
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

#[get("/features")]
async fn list_features(data: web::Data<AppState>) -> impl Responder {
    match data.features.list_all() {
        Ok(features) => {
            let feature_list: Vec<FeatureResponse> = features
                .flags
                .iter()
                .map(|(name, flag)| FeatureResponse {
                    name: name.clone(),
                    enabled: flag.enabled,
                    description: flag.description.clone(),
                })
                .collect();

            HttpResponse::Ok().json(ApiResponse {
                success: true,
                message: format!("{} features found", feature_list.len()),
                data: Some(serde_json::json!({
                    "version": features.version,
                    "features": feature_list
                })),
            })
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

#[post("/features/{name}")]
async fn set_feature(
    path: web::Path<String>,
    req: web::Json<SetFeatureRequest>,
    data: web::Data<AppState>,
) -> impl Responder {
    let name = path.into_inner();
    match data
        .features
        .set_flag(name.clone(), req.enabled, Some(req.description.clone()))
    {
        Ok(_) => HttpResponse::Ok().json(ApiResponse {
            success: true,
            message: format!("Feature '{}' updated", name),
            data: None,
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

pub async fn start_server(
    engine: LsmEngine,
    server_config: ServerConfig,
) -> std::io::Result<()> {
    let engine = Arc::new(engine);
    let features = Arc::new(FeatureClient::new(
        Arc::clone(&engine),
        Duration::from_secs(server_config.feature_cache_ttl_secs),
    ));

    server_config.print_info();
    println!("🚀 Starting server at {}:{}\n", server_config.host, server_config.port);

    let max_json = server_config.max_json_payload_size;
    let max_raw = server_config.max_raw_payload_size;
    let host = server_config.host.clone();
    let port = server_config.port;

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header();

        App::new()
            .wrap(cors)
            .app_data(web::Data::new(AppState {
                engine: Arc::clone(&engine),
                features: Arc::clone(&features),
            }))
            .app_data(web::JsonConfig::default().limit(max_json))
            .app_data(web::PayloadConfig::default().limit(max_raw))
            .service(health)
            .service(get_stats)
            .service(get_stats_all)
            .service(get_key)
            .service(set_key)
            .service(set_batch)
            .service(list_keys)
            .service(search_keys)
            .service(scan_all)
            .service(list_features)
            .service(set_feature)
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}
