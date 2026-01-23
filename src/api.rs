use actix_cors::Cors;
use actix_web::{delete, get, post, web, App, HttpResponse, HttpServer, Responder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::engine::LsmEngine;

/// Estado compartilhado entre threads (Actix usa multi-thread)
pub struct AppState {
    pub engine: Arc<LsmEngine>,
}

/// Request body para SET
#[derive(Deserialize)]
pub struct SetRequest {
    pub key: String,
    pub value: String, // JSON aceita string; convertemos para bytes
}

/// Response padrão
#[derive(Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// GET /health - Healthcheck
#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok().json(ApiResponse {
        success: true,
        message: "LSM-Tree API is running".to_string(),
        data: None,
    })
}

/// GET /stats - Estatísticas do engine
#[get("/stats")]
async fn get_stats(data: web::Data<AppState>) -> impl Responder {
    let stats = data.engine.stats();
    HttpResponse::Ok().json(ApiResponse {
        success: true,
        message: "Stats retrieved".to_string(),
        data: Some(serde_json::json!({ "stats": stats })),
    })
}

/// GET /keys/{key} - Buscar valor por chave
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

/// POST /keys - Inserir ou atualizar chave
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

/// DELETE /keys/{key} - Deletar chave
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

/// GET /keys - Listar todas as chaves
#[get("/keys")]
async fn list_keys(data: web::Data<AppState>) -> impl Responder {
    match data.engine.keys() {
        Ok(keys) => HttpResponse::Ok().json(ApiResponse {
            success: true,
            message: format!("{} keys found", keys.len()),
            data: Some(serde_json::json!({ "keys": keys })),
        }),
        Err(e) => HttpResponse::InternalServerError().json(ApiResponse {
            success: false,
            message: format!("Error: {}", e),
            data: None,
        }),
    }
}

/// GET /scan - Retornar todos os dados
#[get("/scan")]
async fn scan_all(data: web::Data<AppState>) -> impl Responder {
    match data.engine.scan() {
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

/// Inicia o servidor HTTP
pub async fn start_server(engine: LsmEngine, host: &str, port: u16) -> std::io::Result<()> {
    let engine = Arc::new(engine);

    println!("🚀 LSM-Tree REST API iniciando em http://{}:{}", host, port);
    println!("📚 Documentação:");
    println!("   GET  /health          - Healthcheck");
    println!("   GET  /stats           - Estatísticas");
    println!("   GET  /keys            - Listar todas as chaves");
    println!("   GET  /keys/{{key}}      - Buscar valor");
    println!("   POST /keys            - Inserir/atualizar (JSON body)");
    println!("   DELETE /keys/{{key}}    - Deletar chave");
    println!("   GET  /scan            - Retornar todos os dados\n");

    HttpServer::new(move || {
        // CORS para permitir requisições do frontend
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .wrap(cors)
            .app_data(web::Data::new(AppState {
                engine: Arc::clone(&engine),
            }))
            .service(health)
            .service(get_stats)
            .service(get_key)
            .service(set_key)
            .service(delete_key)
            .service(list_keys)
            .service(scan_all)
    })
    .bind((host, port))?
    .run()
    .await
}
