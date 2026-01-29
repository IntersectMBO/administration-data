//! Milestones endpoints

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::Json,
};
use sqlx::PgPool;

use crate::models::v1::{
    ApiResponse, MilestoneResponse, MilestoneRow, MilestonesQuery, PaginatedResponse,
};

/// List all milestones
///
/// Returns a paginated list of milestones across all projects with filtering support.
#[utoipa::path(
    get,
    path = "/api/v1/milestones",
    params(MilestonesQuery),
    responses(
        (status = 200, description = "List of milestones", body = PaginatedResponse<Vec<MilestoneResponse>>)
    ),
    tag = "Milestones"
)]
pub async fn list_milestones(
    Extension(pool): Extension<PgPool>,
    Query(params): Query<MilestonesQuery>,
) -> Result<Json<PaginatedResponse<Vec<MilestoneResponse>>>, StatusCode> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    // Build dynamic query based on filters
    let mut conditions = Vec::new();
    let mut bind_index = 1;

    if params.status.is_some() {
        conditions.push(format!("m.status = ${}", bind_index));
        bind_index += 1;
    }

    if params.project_id.is_some() {
        conditions.push(format!("vc.project_id = ${}", bind_index));
        bind_index += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Determine sort order
    let sort_clause = match params.sort.as_deref() {
        Some("complete_time") => "m.complete_time DESC NULLS LAST",
        Some("disburse_time") => "m.disburse_time DESC NULLS LAST",
        Some("amount") => "m.amount_lovelace DESC NULLS LAST",
        _ => "vc.project_id, m.milestone_order",
    };

    // Get total count
    let count_query = format!(
        r#"
        SELECT COUNT(*)
        FROM treasury.milestones m
        JOIN treasury.vendor_contracts vc ON vc.id = m.vendor_contract_id
        {}
        "#,
        where_clause
    );

    let mut count_q = sqlx::query_as::<_, (i64,)>(&count_query);

    if let Some(ref status) = params.status {
        count_q = count_q.bind(status);
    }
    if let Some(ref project_id) = params.project_id {
        count_q = count_q.bind(project_id);
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
        SELECT
            m.id,
            m.vendor_contract_id,
            m.milestone_id,
            m.milestone_order,
            m.label,
            m.description,
            m.acceptance_criteria,
            m.amount_lovelace,
            m.status,
            m.complete_tx_hash,
            m.complete_time,
            m.complete_description,
            m.evidence,
            m.disburse_tx_hash,
            m.disburse_time,
            m.disburse_amount,
            vc.project_id,
            vc.project_name
        FROM treasury.milestones m
        JOIN treasury.vendor_contracts vc ON vc.id = m.vendor_contract_id
        {}
        ORDER BY {}
        LIMIT ${} OFFSET ${}
        "#,
        where_clause,
        sort_clause,
        bind_index,
        bind_index + 1
    );

    let mut data_q = sqlx::query_as::<_, MilestoneRow>(&data_query);

    if let Some(ref status) = params.status {
        data_q = data_q.bind(status);
    }
    if let Some(ref project_id) = params.project_id {
        data_q = data_q.bind(project_id);
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

    let milestones: Vec<MilestoneResponse> = rows.into_iter().map(MilestoneResponse::from).collect();
    Ok(Json(PaginatedResponse::new(milestones, page, limit, total_count)))
}

/// Get a specific milestone by ID
///
/// Returns detailed information about a specific milestone.
#[utoipa::path(
    get,
    path = "/api/v1/milestones/{id}",
    params(
        ("id" = i32, Path, description = "Milestone database ID")
    ),
    responses(
        (status = 200, description = "Milestone details", body = ApiResponse<MilestoneResponse>),
        (status = 404, description = "Milestone not found")
    ),
    tag = "Milestones"
)]
pub async fn get_milestone(
    Extension(pool): Extension<PgPool>,
    Path(id): Path<i32>,
) -> Result<Json<ApiResponse<MilestoneResponse>>, StatusCode> {
    let row = sqlx::query_as::<_, MilestoneRow>(
        r#"
        SELECT
            m.id,
            m.vendor_contract_id,
            m.milestone_id,
            m.milestone_order,
            m.label,
            m.description,
            m.acceptance_criteria,
            m.amount_lovelace,
            m.status,
            m.complete_tx_hash,
            m.complete_time,
            m.complete_description,
            m.evidence,
            m.disburse_tx_hash,
            m.disburse_time,
            m.disburse_amount,
            vc.project_id,
            vc.project_name
        FROM treasury.milestones m
        JOIN treasury.vendor_contracts vc ON vc.id = m.vendor_contract_id
        WHERE m.id = $1
        "#
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ApiResponse::new(MilestoneResponse::from(row))))
}
