use axum::{
    extract::Extension,
    http::Method,
    routing::get,
    Router,
};
use sqlx::PgPool;
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

mod models;
mod openapi;
mod routes;
mod services;

use openapi::ApiDoc;
use services::run_sync_loop;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Load environment variables
    dotenvy::dotenv().ok();

    // Database connection with retry logic
    // Default to port 5433 to avoid conflict with local PostgreSQL on 5432
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost:5433/administration_data".to_string());

    tracing::info!("Connecting to database: {}", database_url);

    // Retry connection up to 5 times with exponential backoff
    let mut pool = None;
    for attempt in 1..=5 {
        match PgPool::connect(&database_url).await {
            Ok(p) => {
                pool = Some(p);
                break;
            }
            Err(e) => {
                if attempt == 5 {
                    tracing::error!("Failed to connect to database after 5 attempts: {}", e);
                    return Err(e.into());
                }
                let delay = std::time::Duration::from_secs(2_u64.pow(attempt));
                tracing::warn!(
                    "Database connection attempt {} failed: {}. Retrying in {:?}...",
                    attempt,
                    e,
                    delay
                );
                tokio::time::sleep(delay).await;
            }
        }
    }

    let pool = pool.expect("Pool should be initialized after retries");
    tracing::info!("Database connection established");

    // Spawn background sync task
    let sync_pool = pool.clone();
    tokio::spawn(async move {
        run_sync_loop(sync_pool).await;
    });
    tracing::info!("Background sync task started");

    // Build application routes
    let app = Router::new()
        // Health check
        .route("/health", get(health_check))
        // OpenAPI / Swagger UI
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
        // V1 API routes
        .nest("/api/v1", routes::v1::router())
        .layer(Extension(pool))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET])
                .allow_headers(Any),
        );

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("Server listening on {}", addr);
    tracing::info!("Swagger UI available at http://localhost:8080/docs");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> &'static str {
    "OK"
}
