//! V1 API Routes
//!
//! New API design with:
//! - Consistent response envelopes
//! - Pagination support
//! - Both lovelace and ADA amounts
//! - Raw and parsed metadata

pub mod treasury;
pub mod vendor_contracts;
pub mod milestones;
pub mod events;
pub mod statistics;

use axum::{routing::get, Router};

/// Create the v1 API router
pub fn router() -> Router {
    Router::new()
        // Status endpoint
        .route("/status", get(status::get_status))
        // Treasury endpoints
        .route("/treasury", get(treasury::get_treasury))
        .route("/treasury/utxos", get(treasury::get_treasury_utxos))
        .route("/treasury/events", get(treasury::get_treasury_events))
        // Vendor contracts endpoints
        .route("/vendor-contracts", get(vendor_contracts::list_vendor_contracts))
        .route("/vendor-contracts/:project_id", get(vendor_contracts::get_vendor_contract))
        .route("/vendor-contracts/:project_id/milestones", get(vendor_contracts::get_vendor_contract_milestones))
        .route("/vendor-contracts/:project_id/events", get(vendor_contracts::get_vendor_contract_events))
        .route("/vendor-contracts/:project_id/utxos", get(vendor_contracts::get_vendor_contract_utxos))
        // Milestones endpoints
        .route("/milestones", get(milestones::list_milestones))
        .route("/milestones/:id", get(milestones::get_milestone))
        // Events endpoints
        .route("/events", get(events::list_events))
        .route("/events/recent", get(events::get_recent_events))
        .route("/events/:tx_hash", get(events::get_event))
        // Statistics endpoint
        .route("/statistics", get(statistics::get_statistics))
}

pub mod status {
    use axum::{extract::Extension, http::StatusCode, response::Json};
    use sqlx::PgPool;

    use crate::models::v1::{ApiResponse, StatusResponse};

    /// Get API status and sync information
    #[utoipa::path(
        get,
        path = "/api/v1/status",
        responses(
            (status = 200, description = "API status", body = ApiResponse<StatusResponse>)
        ),
        tag = "Status"
    )]
    pub async fn get_status(
        Extension(pool): Extension<PgPool>,
    ) -> Result<Json<ApiResponse<StatusResponse>>, StatusCode> {
        // Get sync status
        let sync_row = sqlx::query_as::<_, (Option<i64>, Option<i64>, Option<chrono::DateTime<chrono::Utc>>)>(
            "SELECT last_slot, last_block, updated_at FROM treasury.sync_status WHERE sync_type = 'events'"
        )
        .fetch_optional(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Database query error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let (last_slot, last_block, last_sync_time) = sync_row.unwrap_or((None, None, None));

        // Get event count
        let (total_events,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM treasury.events")
            .fetch_one(&pool)
            .await
            .map_err(|e| {
                tracing::error!("Database query error: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        // Get vendor contract count
        let (total_vendor_contracts,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM treasury.vendor_contracts")
            .fetch_one(&pool)
            .await
            .map_err(|e| {
                tracing::error!("Database query error: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        Ok(Json(ApiResponse::new(StatusResponse {
            api_version: "1.0.0".to_string(),
            database_connected: true,
            last_sync_slot: last_slot,
            last_sync_block: last_block,
            last_sync_time: last_sync_time.map(|t| t.timestamp()),
            total_events,
            total_vendor_contracts,
        })))
    }
}
