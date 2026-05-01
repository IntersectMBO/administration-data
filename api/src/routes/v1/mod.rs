//! V1 API Routes
//!
//! Conventions:
//! - Consistent response envelopes (`ApiResponse` / `PaginatedResponse`).
//! - Errors return a parallel `ApiErrorBody` envelope.
//! - Pagination on every list endpoint.
//! - Amounts in lovelace only (1 ADA = 1,000,000 lovelace).
//! - On-chain block times are paired `{unix, iso}`; server-side timestamps stay ISO.
//! - Raw and parsed metadata.

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
    use axum::{extract::Extension, response::Json};
    use sqlx::PgPool;
    use std::collections::HashMap;

    use crate::errors::ApiError;
    use crate::models::time::ChainTime;
    use crate::models::v1::{
        ApiResponse, ChainStatus, DatabaseStatus, StatusResponse, SyncStatusBlock, TotalsBlock,
    };

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
    ) -> Result<Json<ApiResponse<StatusResponse>>, ApiError> {
        // Sync heartbeat (server-side ISO)
        let heartbeat: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            "SELECT updated_at FROM treasury.sync_status WHERE sync_type = 'events'",
        )
        .fetch_optional(&pool)
        .await?
        .flatten();

        // Most recent TOM event processed (on-chain ChainTime)
        let last_event_block_time: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(block_time) FROM treasury.events",
        )
        .fetch_one(&pool)
        .await
        .unwrap_or(None);

        // Indexer cursor + block time of that block
        let cursor: Option<(Option<i64>, Option<i64>, Option<i64>)> = sqlx::query_as(
            r#"
            SELECT c.block_number, c.slot, b.block_time
            FROM yaci_store.cursor_ c
            LEFT JOIN yaci_store.block b ON b.number = c.block_number
            ORDER BY c.slot DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&pool)
        .await
        .unwrap_or(None);
        let (indexer_block, indexer_slot, indexer_block_time) = cursor.unwrap_or((None, None, None));

        // Totals
        let (total_events,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM treasury.events")
            .fetch_one(&pool)
            .await?;

        let (total_vendor_contracts,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM treasury.vendor_contracts")
                .fetch_one(&pool)
                .await?;

        let by_type_rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT event_type, COUNT(*) FROM treasury.events GROUP BY event_type ORDER BY 1",
        )
        .fetch_all(&pool)
        .await?;
        let events_by_type: HashMap<String, i64> = by_type_rows.into_iter().collect();

        Ok(Json(ApiResponse::new(StatusResponse {
            api_version: "1.1.0".to_string(),
            database: DatabaseStatus {
                connected: true,
                checked_at: chrono::Utc::now(),
            },
            sync: SyncStatusBlock {
                heartbeat,
                last_event_processed: ChainTime::maybe_from_secs(last_event_block_time),
            },
            chain: ChainStatus {
                indexer_block,
                indexer_slot,
                indexer_time: ChainTime::maybe_from_secs(indexer_block_time),
            },
            totals: TotalsBlock {
                events: total_events,
                vendor_contracts: total_vendor_contracts,
                events_by_type,
            },
        })))
    }
}
