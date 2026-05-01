//! Treasury endpoints

use axum::{
    extract::{Extension, Query},
    response::Json,
};
use sqlx::PgPool;

use crate::errors::ApiError;
use crate::models::v1::{
    ApiResponse, EventResponse, EventWithContextRow, EventsQuery, PaginatedResponse,
    PaginationQuery, TreasuryResponse, TreasurySummaryRow, UtxoResponse, UtxoRow,
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
        (status = 404, description = "No treasury found", body = crate::errors::ApiErrorBody)
    ),
    tag = "Treasury"
)]
pub async fn get_treasury(
    Extension(pool): Extension<PgPool>,
) -> Result<Json<ApiResponse<TreasuryResponse>>, ApiError> {
    let row = sqlx::query_as::<_, TreasurySummaryRow>(
        r#"
        SELECT *
        FROM treasury.v_treasury_summary
        LIMIT 1
        "#,
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("treasury not found".into()))?;

    Ok(Json(ApiResponse::new(TreasuryResponse::from(row))))
}

/// Get treasury UTXOs (paginated)
///
/// Returns all unspent UTXOs at the treasury contract address.
#[utoipa::path(
    get,
    path = "/api/v1/treasury/utxos",
    params(PaginationQuery),
    responses(
        (status = 200, description = "Treasury UTXOs", body = PaginatedResponse<Vec<UtxoResponse>>),
        (status = 404, description = "No treasury found", body = crate::errors::ApiErrorBody)
    ),
    tag = "Treasury"
)]
pub async fn get_treasury_utxos(
    Extension(pool): Extension<PgPool>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<PaginatedResponse<Vec<UtxoResponse>>>, ApiError> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    // First get the treasury contract address
    let treasury = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT contract_address FROM treasury.treasury_contracts LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound("treasury not found".into()))?;

    let address = treasury
        .0
        .ok_or_else(|| ApiError::NotFound("treasury contract_address not yet known".into()))?;

    let (total_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM treasury.utxo_history WHERE address = $1 AND NOT spent",
    )
    .bind(&address)
    .fetch_one(&pool)
    .await?;

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
        FROM treasury.utxo_history
        WHERE address = $1 AND NOT spent
        ORDER BY slot DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&address)
    .bind(limit_i64)
    .bind(offset)
    .fetch_all(&pool)
    .await?;

    let utxos: Vec<UtxoResponse> = rows.into_iter().map(UtxoResponse::from).collect();
    Ok(Json(PaginatedResponse::new(utxos, page, limit, total_count)))
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
) -> Result<Json<PaginatedResponse<Vec<EventResponse>>>, ApiError> {
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
        WHERE e.event_type = ANY($1) AND e.project_db_id IS NULL
        "#,
    )
    .bind(&treasury_event_types)
    .fetch_one(&pool)
    .await?;

    let rows = sqlx::query_as::<_, EventWithContextRow>(
        r#"
        SELECT *
        FROM treasury.v_events_with_context
        WHERE event_type = ANY($1) AND project_id IS NULL
        ORDER BY block_time DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&treasury_event_types)
    .bind(limit_i64)
    .bind(offset)
    .fetch_all(&pool)
    .await?;

    let events: Vec<EventResponse> = rows.into_iter().map(EventResponse::from).collect();
    Ok(Json(PaginatedResponse::new(events, page, limit, total_count)))
}
