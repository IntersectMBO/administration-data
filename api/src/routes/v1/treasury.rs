//! Treasury endpoints

use axum::{
    extract::{Extension, Query},
    http::StatusCode,
    response::Json,
};
use sqlx::PgPool;

use crate::models::v1::{
    ApiResponse, EventResponse, EventWithContextRow, EventsQuery, PaginatedResponse,
    TreasuryResponse, TreasurySummaryRow, UtxoResponse, UtxoRow,
};

/// Get treasury contract details
///
/// Returns the treasury contract with statistics and financial summary.
/// Since this is a single-treasury deployment, returns the first treasury.
#[utoipa::path(
    get,
    path = "/api/v1/treasury",
    responses(
        (status = 200, description = "Treasury details", body = ApiResponse<TreasuryResponse>),
        (status = 404, description = "No treasury found")
    ),
    tag = "Treasury"
)]
pub async fn get_treasury(
    Extension(pool): Extension<PgPool>,
) -> Result<Json<ApiResponse<TreasuryResponse>>, StatusCode> {
    let row = sqlx::query_as::<_, TreasurySummaryRow>(
        r#"
        SELECT *
        FROM treasury.v_treasury_summary
        LIMIT 1
        "#
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ApiResponse::new(TreasuryResponse::from(row))))
}

/// Get treasury UTXOs
///
/// Returns all unspent UTXOs at the treasury contract address.
#[utoipa::path(
    get,
    path = "/api/v1/treasury/utxos",
    responses(
        (status = 200, description = "Treasury UTXOs", body = ApiResponse<Vec<UtxoResponse>>),
        (status = 404, description = "No treasury found")
    ),
    tag = "Treasury"
)]
pub async fn get_treasury_utxos(
    Extension(pool): Extension<PgPool>,
) -> Result<Json<ApiResponse<Vec<UtxoResponse>>>, StatusCode> {
    // First get the treasury contract address
    let treasury = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT contract_address FROM treasury.treasury_contracts LIMIT 1"
    )
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    let address = treasury.0.ok_or(StatusCode::NOT_FOUND)?;

    let rows = sqlx::query_as::<_, UtxoRow>(
        r#"
        SELECT
            tx_hash,
            output_index,
            address,
            address_type,
            lovelace_amount,
            slot,
            block_number
        FROM treasury.utxos
        WHERE address = $1 AND NOT spent
        ORDER BY slot DESC
        "#
    )
    .bind(&address)
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let utxos: Vec<UtxoResponse> = rows.into_iter().map(UtxoResponse::from).collect();
    Ok(Json(ApiResponse::new(utxos)))
}

/// Get treasury-level events
///
/// Returns events that are at the treasury level (publish, initialize, sweep).
#[utoipa::path(
    get,
    path = "/api/v1/treasury/events",
    params(EventsQuery),
    responses(
        (status = 200, description = "Treasury events", body = PaginatedResponse<Vec<EventResponse>>)
    ),
    tag = "Treasury"
)]
pub async fn get_treasury_events(
    Extension(pool): Extension<PgPool>,
    Query(params): Query<EventsQuery>,
) -> Result<Json<PaginatedResponse<Vec<EventResponse>>>, StatusCode> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    // Treasury-level event types
    let treasury_event_types = vec!["publish", "initialize", "sweep", "reorganize"];

    // Get total count
    let (total_count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM treasury.events e
        JOIN treasury.treasury_contracts tc ON tc.id = e.treasury_id
        WHERE e.event_type = ANY($1) AND e.vendor_contract_id IS NULL
        "#
    )
    .bind(&treasury_event_types)
    .fetch_one(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Get events
    let rows = sqlx::query_as::<_, EventWithContextRow>(
        r#"
        SELECT *
        FROM treasury.v_events_with_context
        WHERE event_type = ANY($1) AND project_id IS NULL
        ORDER BY block_time DESC
        LIMIT $2 OFFSET $3
        "#
    )
    .bind(&treasury_event_types)
    .bind(limit_i64)
    .bind(offset)
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let events: Vec<EventResponse> = rows.into_iter().map(EventResponse::from).collect();
    Ok(Json(PaginatedResponse::new(events, page, limit, total_count)))
}
