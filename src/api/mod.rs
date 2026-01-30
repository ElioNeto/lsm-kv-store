use actix_cors::Cors;
use actix_web::{delete, get, post, web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration; // ADICIONAR

use crate::engine::LsmEngine;
use crate::features::FeatureClient; // CORRIGIR (remover FeatureFlag)

// Estado compartilhado entre threads
pub struct AppState {
    pub engine: Arc<LsmEngine>,
    pub features: Arc<FeatureClient>,
}

// Request body para SET
#[derive(Deserialize)]
pub struct SetRequest {
    pub key: String,
    pub value: String,
}

// Request body para SET BATCH
#[derive(Deserialize)]
pub struct BatchSetRequest {
    pub records: Vec<SetRequest>,
}

// Request body para DELETE BATCH
#[derive(Deserialize)]
pub struct BatchDeleteRequest {
    pub keys: Vec<String>,
}

// Query params para busca
#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String, // query string (substring)
    #[serde(default)]
    pub prefix: bool, // se true, busca por prefixo; se false, substring
}

// Response padrão
#[derive(Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// Request body para features
#[derive(Deserialize)]
pub struct SetFeatureRequest {
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
}

// Response para features
#[derive(Serialize)]
pub struct FeatureResponse {
    pub name: String,
    pub enabled: bool,
    pub description: String,
}

// GET /health - Healthcheck
#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().json(ApiResponse {
        success: true,
        message: "LSM-Tree API is running".to_string(),
        data: None,
    })
}

// GET /stats - Estatísticas do engine
#[get("/stats")]
async fn get_stats(data: web::Data<AppState>) -> impl Responder {
    let stats = data.engine.stats();
    HttpResponse::Ok().json(ApiResponse {
        success: true,
        message: "Stats retrieved".to_string(),
        data: Some(serde_json::json!({ "stats": stats })),
    })
}

// GET /stats/all - Estatísticas completas
#[get("/stats/all")]
async fn get_stats_all(data: web::Data<AppState>) -> impl Responder {
    let stats = data.engine.stats_all();
    HttpResponse::Ok().json(ApiResponse {
        success: true,
        message: "Stats retrieved".to_string(),
        data: Some(serde_json::json!({ "stats": stats })),
    })
}

// GET /keys/{key} - Buscar valor por chave
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

// POST /keys - Inserir ou atualizar chave
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

// POST /keys/batch - Inserir múltiplas chaves
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

// DELETE /keys/{key} - Deletar chave
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

// DELETE /keys/batch - Deletar múltiplas chaves
#[delete("/keys/batch")]
async fn delete_batch(
    req: web::Json<BatchDeleteRequest>,
    data: web::Data<AppState>,
) -> impl Responder {
    match data.engine.delete_batch(req.keys.clone()) {
        Ok(count) => HttpResponse::Ok().json(ApiResponse {
            success: true,
            message: format!("{} keys deleted successfully", count),
            data: Some(serde_json::json!({ "count": count })),
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

// GET /keys - Listar todas as chaves (FILTRADO - sem feature:*)
#[get("/keys")]
async fn list_keys(data: web::Data<AppState>) -> impl Responder {
    match data.engine.keys() {
        Ok(keys) => {
            // Filtrar chaves que começam com "feature:"
            let filtered_keys: Vec<String> = keys
                .into_iter()
                .filter(|k| !k.starts_with("feature:"))
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

// GET /keys/search?q=...&prefix=false - Buscar por substring/prefixo
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
                .map(|(k, v)| {
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

// GET /scan - Retornar todos os dados (FILTRADO - sem feature:*)
#[get("/scan")]
async fn scan_all(data: web::Data<AppState>) -> impl Responder {
    match data.engine.scan() {
        Ok(records) => {
            // Filtrar registros com chave feature:*
            let records_json: Vec<serde_json::Value> = records
                .into_iter()
                .filter(|(k, _)| !k.starts_with("feature:"))
                .map(|(k, v)| {
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

// ==================== FEATURE FLAGS ENDPOINTS ====================

// GET /features - Listar todas as features
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

// GET /features/{name} - Verificar se uma feature está habilitada
#[get("/features/{name}")]
async fn get_feature(path: web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    let name = path.into_inner();

    match data.features.is_enabled(&name) {
        Ok(enabled) => HttpResponse::Ok().json(ApiResponse {
            success: true,
            message: "Feature retrieved".to_string(),
            data: Some(serde_json::json!({
                "name": name,
                "enabled": enabled
            })),
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

// POST /features/{name} - Criar ou atualizar feature
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
            message: format!("Feature '{}' updated successfully", name),
            data: Some(serde_json::json!({
                "name": name,
                "enabled": req.enabled
            })),
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

// DELETE /features/{name} - Remover feature
#[delete("/features/{name}")]
async fn delete_feature(path: web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    let name = path.into_inner();

    match data.features.remove_flag(&name) {
        Ok(removed) => {
            if removed {
                HttpResponse::Ok().json(ApiResponse {
                    success: true,
                    message: format!("Feature '{}' deleted successfully", name),
                    data: None,
                })
            } else {
                HttpResponse::NotFound().json(ApiResponse {
                    success: false,
                    message: format!("Feature '{}' not found", name),
                    data: None,
                })
            }
        }
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

// Inicia o servidor HTTP
pub async fn start_server(engine: LsmEngine, host: &str, port: u16) -> std::io::Result<()> {
    let engine = Arc::new(engine);
    let features = Arc::new(FeatureClient::new(
        Arc::clone(&engine),
        Duration::from_secs(10), // Cache de 10 segundos
    ));

    println!(
        "\n🚀 LSM-Tree REST API iniciando em http://{}:{}",
        host, port
    );
    println!("\n📚 Documentação:");
    println!("  GET  /health            - Healthcheck");
    println!("  GET  /stats             - Estatísticas");
    println!("  GET  /stats_all         - Estatísticas completas");
    println!("  GET  /keys              - Listar chaves (exceto feature:*)");
    println!("  GET  /keys/{{key}}        - Buscar valor");
    println!("  GET  /keys/search?q=... - Buscar por substring/prefixo");
    println!("  POST /keys              - Inserir/atualizar");
    println!("  POST /keys/batch        - Inserir múltiplos");
    println!("  DELETE /keys/{{key}}     - Deletar chave");
    println!("  DELETE /keys/batch      - Deletar múltiplas");
    println!("  GET  /scan              - Scan completo (exceto feature:*)");
    println!("\n🚩 Feature Flags:");
    println!("  GET    /features        - Listar todas as features");
    println!("  GET    /features/{{name}} - Verificar feature");
    println!("  POST   /features/{{name}} - Criar/atualizar feature");
    println!("  DELETE /features/{{name}} - Remover feature");

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .app_data(web::Data::new(AppState {
                engine: Arc::clone(&engine),
                features: Arc::clone(&features),
            }))
            .app_data(web::JsonConfig::default().limit(20 * 1024 * 1024))
            // Endpoints gerais
            .service(health)
            .service(get_stats)
            .service(get_stats_all)
            .service(search_keys)
            .service(get_key)
            .service(set_key)
            .service(set_batch)
            .service(delete_key)
            .service(delete_batch)
            .service(list_keys)
            .service(scan_all)
            // Feature flags
            .service(list_features)
            .service(get_feature)
            .service(set_feature)
            .service(delete_feature)
    })
    .bind((host, port))?
    .run()
    .await
}
