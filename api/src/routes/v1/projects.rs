//! Vendor Contracts (Projects) endpoints

use axum::{
    extract::{Extension, Path, Query},
    response::Json,
};
use sqlx::PgPool;

use crate::errors::ApiError;
use crate::models::v1::{
    ApiResponse, EventResponse, EventWithContextRow, MilestoneResponse, MilestoneRow,
    PaginatedResponse, PaginationQuery, ProjectEventsQuery, UtxoResponse, UtxoRow,
    ProjectDetail, ProjectSummary, ProjectSummaryRow, ProjectsQuery,
};

/// List all vendor contracts
///
/// Returns a paginated list of vendor contracts with filtering and search support.
#[utoipa::path(
    get,
    path = "/api/v1/projects",
    params(ProjectsQuery),
    responses(
        (status = 200, description = "List of vendor contracts", body = PaginatedResponse<Vec<ProjectSummary>>)
    ),
    tag = "Projects"
)]
pub async fn list_projects(
    Extension(pool): Extension<PgPool>,
    Query(params): Query<ProjectsQuery>,
) -> Result<Json<PaginatedResponse<Vec<ProjectSummary>>>, ApiError> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    let mut conditions = Vec::new();
    let mut bind_index = 1;

    if params.status.is_some() {
        conditions.push(format!("status = ${}", bind_index));
        bind_index += 1;
    }

    if params.search.is_some() {
        conditions.push(format!(
            "(project_id ILIKE ${0} OR project_name ILIKE ${0} OR description ILIKE ${0})",
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

    let count_query = format!(
        "SELECT COUNT(*) FROM treasury.v_projects_summary {}",
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

    let (total_count,) = count_q.fetch_one(&pool).await?;

    let data_query = format!(
        r#"
        SELECT *
        FROM treasury.v_projects_summary
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

    let mut data_q = sqlx::query_as::<_, ProjectSummaryRow>(&data_query);

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

    let rows = data_q.bind(limit_i64).bind(offset).fetch_all(&pool).await?;

    let contracts: Vec<ProjectSummary> = rows.into_iter().map(ProjectSummary::from).collect();
    Ok(Json(PaginatedResponse::new(contracts, page, limit, total_count)))
}

/// Get a specific vendor contract by project ID
#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}",
    params(
        ("project_id" = String, Path, description = "Project identifier (e.g., EC-0008-25)")
    ),
    responses(
        (status = 200, description = "Vendor contract details", body = ApiResponse<ProjectDetail>),
        (status = 404, description = "Vendor contract not found", body = crate::errors::ApiErrorBody)
    ),
    tag = "Projects"
)]
pub async fn get_project(
    Extension(pool): Extension<PgPool>,
    Path(project_id): Path<String>,
) -> Result<Json<ApiResponse<ProjectDetail>>, ApiError> {
    let row = sqlx::query_as::<_, ProjectSummaryRow>(
        r#"
        SELECT *
        FROM treasury.v_projects_summary
        WHERE project_id = $1
        "#,
    )
    .bind(&project_id)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("vendor contract `{}` not found", project_id)))?;

    Ok(Json(ApiResponse::new(ProjectDetail::from(row))))
}

/// Get milestones for a vendor contract (paginated)
#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/milestones",
    params(
        ("project_id" = String, Path, description = "Project identifier"),
        PaginationQuery
    ),
    responses(
        (status = 200, description = "Project milestones", body = PaginatedResponse<Vec<MilestoneResponse>>),
        (status = 404, description = "Vendor contract not found", body = crate::errors::ApiErrorBody)
    ),
    tag = "Projects"
)]
pub async fn get_project_milestones(
    Extension(pool): Extension<PgPool>,
    Path(project_id): Path<String>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<PaginatedResponse<Vec<MilestoneResponse>>>, ApiError> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    let exists = sqlx::query_as::<_, (i32,)>(
        "SELECT id FROM treasury.projects WHERE project_id = $1",
    )
    .bind(&project_id)
    .fetch_optional(&pool)
    .await?;

    if exists.is_none() {
        return Err(ApiError::NotFound(format!(
            "vendor contract `{}` not found",
            project_id
        )));
    }

    let (total_count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM treasury.milestones m
        JOIN treasury.projects vc ON vc.id = m.project_db_id
        WHERE vc.project_id = $1 AND NOT m.archived
        "#,
    )
    .bind(&project_id)
    .fetch_one(&pool)
    .await?;

    let rows = sqlx::query_as::<_, MilestoneRow>(
        r#"
        SELECT
            m.id, m.project_db_id, m.milestone_id, m.milestone_order,
            m.label, m.description, m.acceptance_criteria,
            m.amount_lovelace, m.time_limit,
            m.withdrawn, m.evidence_provided, m.paused, m.archived,
            m.complete_tx_hash, m.complete_time, m.complete_description, m.evidence,
            m.withdraw_tx_hash, m.withdraw_time, m.withdraw_amount,
            m.archived_by_tx_hash, m.archived_at, m.superseded_by,
            (SELECT tx_hash FROM treasury.events WHERE milestone_id = m.id AND event_type = 'pause' ORDER BY block_time DESC LIMIT 1) AS last_pause_tx_hash,
            (SELECT block_time FROM treasury.events WHERE milestone_id = m.id AND event_type = 'pause' ORDER BY block_time DESC LIMIT 1) AS last_pause_time,
            (SELECT tx_hash FROM treasury.events WHERE milestone_id = m.id AND event_type = 'resume' ORDER BY block_time DESC LIMIT 1) AS last_resume_tx_hash,
            (SELECT block_time FROM treasury.events WHERE milestone_id = m.id AND event_type = 'resume' ORDER BY block_time DESC LIMIT 1) AS last_resume_time,
            vc.project_id, vc.project_name
        FROM treasury.milestones m
        JOIN treasury.projects vc ON vc.id = m.project_db_id
        WHERE vc.project_id = $1 AND NOT m.archived
        ORDER BY m.milestone_order
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&project_id)
    .bind(limit_i64)
    .bind(offset)
    .fetch_all(&pool)
    .await?;

    let milestones: Vec<MilestoneResponse> = rows.into_iter().map(MilestoneResponse::from).collect();
    Ok(Json(PaginatedResponse::new(milestones, page, limit, total_count)))
}

/// Get events for a vendor contract
#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/events",
    params(
        ("project_id" = String, Path, description = "Project identifier"),
        ProjectEventsQuery
    ),
    responses(
        (status = 200, description = "Project events", body = PaginatedResponse<Vec<EventResponse>>),
        (status = 404, description = "Vendor contract not found", body = crate::errors::ApiErrorBody)
    ),
    tag = "Projects"
)]
pub async fn get_project_events(
    Extension(pool): Extension<PgPool>,
    Path(project_id): Path<String>,
    Query(params): Query<ProjectEventsQuery>,
) -> Result<Json<PaginatedResponse<Vec<EventResponse>>>, ApiError> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    let exists = sqlx::query_as::<_, (i32,)>(
        "SELECT id FROM treasury.projects WHERE project_id = $1",
    )
    .bind(&project_id)
    .fetch_optional(&pool)
    .await?;

    if exists.is_none() {
        return Err(ApiError::NotFound(format!(
            "vendor contract `{}` not found",
            project_id
        )));
    }

    let (total_count, rows) = if let Some(ref event_type) = params.event_type {
        let (count,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM treasury.v_events_with_context
            WHERE project_id = $1 AND event_type = $2
            "#,
        )
        .bind(&project_id)
        .bind(event_type)
        .fetch_one(&pool)
        .await?;

        let rows = sqlx::query_as::<_, EventWithContextRow>(
            r#"
            SELECT *
            FROM treasury.v_events_with_context
            WHERE project_id = $1 AND event_type = $2
            ORDER BY block_time DESC
            LIMIT $3 OFFSET $4
            "#,
        )
        .bind(&project_id)
        .bind(event_type)
        .bind(limit_i64)
        .bind(offset)
        .fetch_all(&pool)
        .await?;

        (count, rows)
    } else {
        let (count,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM treasury.v_events_with_context
            WHERE project_id = $1
            "#,
        )
        .bind(&project_id)
        .fetch_one(&pool)
        .await?;

        let rows = sqlx::query_as::<_, EventWithContextRow>(
            r#"
            SELECT *
            FROM treasury.v_events_with_context
            WHERE project_id = $1
            ORDER BY block_time DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(&project_id)
        .bind(limit_i64)
        .bind(offset)
        .fetch_all(&pool)
        .await?;

        (count, rows)
    };

    let events: Vec<EventResponse> = rows.into_iter().map(EventResponse::from).collect();
    Ok(Json(PaginatedResponse::new(events, page, limit, total_count)))
}

/// Get UTXOs for a vendor contract (paginated)
#[utoipa::path(
    get,
    path = "/api/v1/projects/{project_id}/utxos",
    params(
        ("project_id" = String, Path, description = "Project identifier"),
        PaginationQuery
    ),
    responses(
        (status = 200, description = "Project UTXOs", body = PaginatedResponse<Vec<UtxoResponse>>),
        (status = 404, description = "Vendor contract not found", body = crate::errors::ApiErrorBody)
    ),
    tag = "Projects"
)]
pub async fn get_project_utxos(
    Extension(pool): Extension<PgPool>,
    Path(project_id): Path<String>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<PaginatedResponse<Vec<UtxoResponse>>>, ApiError> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    let exists = sqlx::query_as::<_, (i32,)>(
        "SELECT id FROM treasury.projects WHERE project_id = $1",
    )
    .bind(&project_id)
    .fetch_optional(&pool)
    .await?;

    if exists.is_none() {
        return Err(ApiError::NotFound(format!(
            "vendor contract `{}` not found",
            project_id
        )));
    }

    let (total_count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM treasury.utxo_history u
        JOIN treasury.projects vc ON vc.id = u.project_db_id
        WHERE vc.project_id = $1 AND NOT u.spent
        "#,
    )
    .bind(&project_id)
    .fetch_one(&pool)
    .await?;

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
        FROM treasury.utxo_history u
        JOIN treasury.projects vc ON vc.id = u.project_db_id
        WHERE vc.project_id = $1 AND NOT u.spent
        ORDER BY u.slot DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&project_id)
    .bind(limit_i64)
    .bind(offset)
    .fetch_all(&pool)
    .await?;

    let utxos: Vec<UtxoResponse> = rows.into_iter().map(UtxoResponse::from).collect();
    Ok(Json(PaginatedResponse::new(utxos, page, limit, total_count)))
}
