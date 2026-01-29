//! Events endpoints

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::Json,
};
use sqlx::PgPool;

use crate::models::v1::{
    ApiResponse, EventResponse, EventWithContextRow, EventsQuery, PaginatedResponse,
    RecentEventsQuery,
};

/// List all events
///
/// Returns a paginated list of all events with filtering support.
#[utoipa::path(
    get,
    path = "/api/v1/events",
    params(EventsQuery),
    responses(
        (status = 200, description = "List of events", body = PaginatedResponse<Vec<EventResponse>>)
    ),
    tag = "Events"
)]
pub async fn list_events(
    Extension(pool): Extension<PgPool>,
    Query(params): Query<EventsQuery>,
) -> Result<Json<PaginatedResponse<Vec<EventResponse>>>, StatusCode> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    // Build dynamic query based on filters
    let mut conditions = Vec::new();
    let mut bind_index = 1;

    if params.event_type.is_some() {
        conditions.push(format!("event_type = ${}", bind_index));
        bind_index += 1;
    }

    if params.project_id.is_some() {
        conditions.push(format!("project_id = ${}", bind_index));
        bind_index += 1;
    }

    if params.from_time.is_some() {
        conditions.push(format!("block_time >= ${}", bind_index));
        bind_index += 1;
    }

    if params.to_time.is_some() {
        conditions.push(format!("block_time <= ${}", bind_index));
        bind_index += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Get total count
    let count_query = format!(
        "SELECT COUNT(*) FROM treasury.v_events_with_context {}",
        where_clause
    );

    let mut count_q = sqlx::query_as::<_, (i64,)>(&count_query);

    if let Some(ref event_type) = params.event_type {
        count_q = count_q.bind(event_type);
    }
    if let Some(ref project_id) = params.project_id {
        count_q = count_q.bind(project_id);
    }
    if let Some(from_time) = params.from_time {
        count_q = count_q.bind(from_time);
    }
    if let Some(to_time) = params.to_time {
        count_q = count_q.bind(to_time);
    }

    let (total_count,) = count_q
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Database query error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Get data
    let data_query = format!(
        r#"
        SELECT *
        FROM treasury.v_events_with_context
        {}
        ORDER BY block_time DESC
        LIMIT ${} OFFSET ${}
        "#,
        where_clause,
        bind_index,
        bind_index + 1
    );

    let mut data_q = sqlx::query_as::<_, EventWithContextRow>(&data_query);

    if let Some(ref event_type) = params.event_type {
        data_q = data_q.bind(event_type);
    }
    if let Some(ref project_id) = params.project_id {
        data_q = data_q.bind(project_id);
    }
    if let Some(from_time) = params.from_time {
        data_q = data_q.bind(from_time);
    }
    if let Some(to_time) = params.to_time {
        data_q = data_q.bind(to_time);
    }

    let rows = data_q
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

/// Get recent events
///
/// Returns events from the last N hours (default 24). Useful for activity feeds.
#[utoipa::path(
    get,
    path = "/api/v1/events/recent",
    params(RecentEventsQuery),
    responses(
        (status = 200, description = "Recent events", body = ApiResponse<Vec<EventResponse>>)
    ),
    tag = "Events"
)]
pub async fn get_recent_events(
    Extension(pool): Extension<PgPool>,
    Query(params): Query<RecentEventsQuery>,
) -> Result<Json<ApiResponse<Vec<EventResponse>>>, StatusCode> {
    let hours = params.hours.max(1).min(168); // Max 1 week
    let limit = params.limit.min(100).max(1) as i64;

    // Calculate cutoff time (hours ago from now)
    let cutoff_seconds = (hours as i64) * 3600;

    let rows = if let Some(ref event_type) = params.event_type {
        sqlx::query_as::<_, EventWithContextRow>(
            r#"
            SELECT *
            FROM treasury.v_events_with_context
            WHERE block_time >= (EXTRACT(EPOCH FROM NOW()) - $1)::BIGINT
              AND event_type = $2
            ORDER BY block_time DESC
            LIMIT $3
            "#
        )
        .bind(cutoff_seconds)
        .bind(event_type)
        .bind(limit)
        .fetch_all(&pool)
        .await
    } else {
        sqlx::query_as::<_, EventWithContextRow>(
            r#"
            SELECT *
            FROM treasury.v_events_with_context
            WHERE block_time >= (EXTRACT(EPOCH FROM NOW()) - $1)::BIGINT
            ORDER BY block_time DESC
            LIMIT $2
            "#
        )
        .bind(cutoff_seconds)
        .bind(limit)
        .fetch_all(&pool)
        .await
    };

    let rows = rows.map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let events: Vec<EventResponse> = rows.into_iter().map(EventResponse::from).collect();
    Ok(Json(ApiResponse::new(events)))
}

/// Get a specific event by transaction hash
///
/// Returns detailed information about a specific event.
#[utoipa::path(
    get,
    path = "/api/v1/events/{tx_hash}",
    params(
        ("tx_hash" = String, Path, description = "Transaction hash")
    ),
    responses(
        (status = 200, description = "Event details", body = ApiResponse<EventResponse>),
        (status = 404, description = "Event not found")
    ),
    tag = "Events"
)]
pub async fn get_event(
    Extension(pool): Extension<PgPool>,
    Path(tx_hash): Path<String>,
) -> Result<Json<ApiResponse<EventResponse>>, StatusCode> {
    let row = sqlx::query_as::<_, EventWithContextRow>(
        r#"
        SELECT *
        FROM treasury.v_events_with_context
        WHERE tx_hash = $1
        "#
    )
    .bind(&tx_hash)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ApiResponse::new(EventResponse::from(row))))
}
