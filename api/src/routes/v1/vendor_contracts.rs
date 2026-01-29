//! Vendor Contracts (Projects) endpoints

use axum::{
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::Json,
};
use sqlx::PgPool;

use crate::models::v1::{
    ApiResponse, EventResponse, EventWithContextRow, MilestoneResponse, MilestoneRow,
    PaginatedResponse, ProjectEventsQuery, UtxoResponse, UtxoRow, VendorContractDetail,
    VendorContractSummary, VendorContractSummaryRow, VendorContractsQuery,
};

/// List all vendor contracts
///
/// Returns a paginated list of vendor contracts with filtering and search support.
#[utoipa::path(
    get,
    path = "/api/v1/vendor-contracts",
    params(VendorContractsQuery),
    responses(
        (status = 200, description = "List of vendor contracts", body = PaginatedResponse<Vec<VendorContractSummary>>)
    ),
    tag = "Vendor Contracts"
)]
pub async fn list_vendor_contracts(
    Extension(pool): Extension<PgPool>,
    Query(params): Query<VendorContractsQuery>,
) -> Result<Json<PaginatedResponse<Vec<VendorContractSummary>>>, StatusCode> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    // Build dynamic query based on filters
    let mut conditions = Vec::new();
    let mut bind_index = 1;

    if params.status.is_some() {
        conditions.push(format!("status = ${}", bind_index));
        bind_index += 1;
    }

    if params.search.is_some() {
        conditions.push(format!(
            "(project_id ILIKE ${0} OR project_name ILIKE ${0} OR description ILIKE ${0} OR vendor_name ILIKE ${0})",
            bind_index
        ));
        bind_index += 1;
    }

    if params.from_time.is_some() {
        conditions.push(format!("fund_block_time >= ${}", bind_index));
        bind_index += 1;
    }

    if params.to_time.is_some() {
        conditions.push(format!("fund_block_time <= ${}", bind_index));
        bind_index += 1;
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    // Determine sort order
    let sort_field = match params.sort.as_deref() {
        Some("project_id") => "project_id",
        Some("project_name") => "project_name",
        Some("initial_amount") => "initial_amount_lovelace",
        _ => "fund_block_time",
    };
    let sort_order = match params.order.as_deref() {
        Some("asc") => "ASC",
        _ => "DESC",
    };

    // Get total count
    let count_query = format!(
        "SELECT COUNT(*) FROM treasury.v_vendor_contracts_summary {}",
        where_clause
    );

    let mut count_q = sqlx::query_as::<_, (i64,)>(&count_query);

    if let Some(ref status) = params.status {
        count_q = count_q.bind(status);
    }
    if let Some(ref search) = params.search {
        count_q = count_q.bind(format!("%{}%", search));
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
        FROM treasury.v_vendor_contracts_summary
        {}
        ORDER BY {} {} NULLS LAST
        LIMIT ${} OFFSET ${}
        "#,
        where_clause,
        sort_field,
        sort_order,
        bind_index,
        bind_index + 1
    );

    let mut data_q = sqlx::query_as::<_, VendorContractSummaryRow>(&data_query);

    if let Some(ref status) = params.status {
        data_q = data_q.bind(status);
    }
    if let Some(ref search) = params.search {
        data_q = data_q.bind(format!("%{}%", search));
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

    let contracts: Vec<VendorContractSummary> = rows.into_iter().map(VendorContractSummary::from).collect();
    Ok(Json(PaginatedResponse::new(contracts, page, limit, total_count)))
}

/// Get a specific vendor contract by project ID
///
/// Returns detailed information about a vendor contract including milestones summary and financials.
#[utoipa::path(
    get,
    path = "/api/v1/vendor-contracts/{project_id}",
    params(
        ("project_id" = String, Path, description = "Project identifier (e.g., EC-0008-25)")
    ),
    responses(
        (status = 200, description = "Vendor contract details", body = ApiResponse<VendorContractDetail>),
        (status = 404, description = "Vendor contract not found")
    ),
    tag = "Vendor Contracts"
)]
pub async fn get_vendor_contract(
    Extension(pool): Extension<PgPool>,
    Path(project_id): Path<String>,
) -> Result<Json<ApiResponse<VendorContractDetail>>, StatusCode> {
    let row = sqlx::query_as::<_, VendorContractSummaryRow>(
        r#"
        SELECT *
        FROM treasury.v_vendor_contracts_summary
        WHERE project_id = $1
        "#
    )
    .bind(&project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(ApiResponse::new(VendorContractDetail::from(row))))
}

/// Get milestones for a vendor contract
///
/// Returns all milestones for a specific project.
#[utoipa::path(
    get,
    path = "/api/v1/vendor-contracts/{project_id}/milestones",
    params(
        ("project_id" = String, Path, description = "Project identifier")
    ),
    responses(
        (status = 200, description = "Project milestones", body = ApiResponse<Vec<MilestoneResponse>>),
        (status = 404, description = "Vendor contract not found")
    ),
    tag = "Vendor Contracts"
)]
pub async fn get_vendor_contract_milestones(
    Extension(pool): Extension<PgPool>,
    Path(project_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<MilestoneResponse>>>, StatusCode> {
    // First verify the project exists
    let exists = sqlx::query_as::<_, (i32,)>(
        "SELECT id FROM treasury.vendor_contracts WHERE project_id = $1"
    )
    .bind(&project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if exists.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let rows = sqlx::query_as::<_, MilestoneRow>(
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
        WHERE vc.project_id = $1
        ORDER BY m.milestone_order
        "#
    )
    .bind(&project_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let milestones: Vec<MilestoneResponse> = rows.into_iter().map(MilestoneResponse::from).collect();
    Ok(Json(ApiResponse::new(milestones)))
}

/// Get events for a vendor contract
///
/// Returns paginated event history for a specific project.
#[utoipa::path(
    get,
    path = "/api/v1/vendor-contracts/{project_id}/events",
    params(
        ("project_id" = String, Path, description = "Project identifier"),
        ProjectEventsQuery
    ),
    responses(
        (status = 200, description = "Project events", body = PaginatedResponse<Vec<EventResponse>>),
        (status = 404, description = "Vendor contract not found")
    ),
    tag = "Vendor Contracts"
)]
pub async fn get_vendor_contract_events(
    Extension(pool): Extension<PgPool>,
    Path(project_id): Path<String>,
    Query(params): Query<ProjectEventsQuery>,
) -> Result<Json<PaginatedResponse<Vec<EventResponse>>>, StatusCode> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    // First verify the project exists
    let exists = sqlx::query_as::<_, (i32,)>(
        "SELECT id FROM treasury.vendor_contracts WHERE project_id = $1"
    )
    .bind(&project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if exists.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Build query based on event type filter
    let (total_count, rows) = if let Some(ref event_type) = params.event_type {
        let (count,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM treasury.v_events_with_context
            WHERE project_id = $1 AND event_type = $2
            "#
        )
        .bind(&project_id)
        .bind(event_type)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Database query error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let rows = sqlx::query_as::<_, EventWithContextRow>(
            r#"
            SELECT *
            FROM treasury.v_events_with_context
            WHERE project_id = $1 AND event_type = $2
            ORDER BY block_time DESC
            LIMIT $3 OFFSET $4
            "#
        )
        .bind(&project_id)
        .bind(event_type)
        .bind(limit_i64)
        .bind(offset)
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Database query error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        (count, rows)
    } else {
        let (count,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM treasury.v_events_with_context
            WHERE project_id = $1
            "#
        )
        .bind(&project_id)
        .fetch_one(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Database query error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let rows = sqlx::query_as::<_, EventWithContextRow>(
            r#"
            SELECT *
            FROM treasury.v_events_with_context
            WHERE project_id = $1
            ORDER BY block_time DESC
            LIMIT $2 OFFSET $3
            "#
        )
        .bind(&project_id)
        .bind(limit_i64)
        .bind(offset)
        .fetch_all(&pool)
        .await
        .map_err(|e| {
            tracing::error!("Database query error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        (count, rows)
    };

    let events: Vec<EventResponse> = rows.into_iter().map(EventResponse::from).collect();
    Ok(Json(PaginatedResponse::new(events, page, limit, total_count)))
}

/// Get UTXOs for a vendor contract
///
/// Returns all unspent UTXOs for a specific project.
#[utoipa::path(
    get,
    path = "/api/v1/vendor-contracts/{project_id}/utxos",
    params(
        ("project_id" = String, Path, description = "Project identifier")
    ),
    responses(
        (status = 200, description = "Project UTXOs", body = ApiResponse<Vec<UtxoResponse>>),
        (status = 404, description = "Vendor contract not found")
    ),
    tag = "Vendor Contracts"
)]
pub async fn get_vendor_contract_utxos(
    Extension(pool): Extension<PgPool>,
    Path(project_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<UtxoResponse>>>, StatusCode> {
    // First verify the project exists
    let exists = sqlx::query_as::<_, (i32,)>(
        "SELECT id FROM treasury.vendor_contracts WHERE project_id = $1"
    )
    .bind(&project_id)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if exists.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }

    let rows = sqlx::query_as::<_, UtxoRow>(
        r#"
        SELECT
            u.tx_hash,
            u.output_index,
            u.address,
            u.address_type,
            u.lovelace_amount,
            u.slot,
            u.block_number
        FROM treasury.utxos u
        JOIN treasury.vendor_contracts vc ON vc.id = u.vendor_contract_id
        WHERE vc.project_id = $1 AND NOT u.spent
        ORDER BY u.slot DESC
        "#
    )
    .bind(&project_id)
    .fetch_all(&pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let utxos: Vec<UtxoResponse> = rows.into_iter().map(UtxoResponse::from).collect();
    Ok(Json(ApiResponse::new(utxos)))
}
