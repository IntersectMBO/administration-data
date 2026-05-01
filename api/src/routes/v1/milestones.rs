//! Milestones endpoints

use axum::{
    extract::{Extension, Path, Query},
    response::Json,
};
use sqlx::PgPool;

use crate::errors::ApiError;
use crate::models::v1::{
    ApiResponse, MilestoneResponse, MilestoneRow, MilestonesQuery, PaginatedResponse,
    PaginationQuery,
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
) -> Result<Json<PaginatedResponse<Vec<MilestoneResponse>>>, ApiError> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    // Build dynamic query based on filters
    let mut conditions = Vec::new();
    let mut bind_index = 1;

    // Default to non-archived milestones unless archived filter is explicitly set
    if let Some(archived) = params.archived {
        conditions.push(format!("m.archived = ${}", bind_index));
        bind_index += 1;
        let _ = archived; // used via binding below
    } else {
        conditions.push("NOT m.archived".to_string());
    }

    if params.withdrawn.is_some() {
        conditions.push(format!("m.withdrawn = ${}", bind_index));
        bind_index += 1;
    }

    if params.evidence_provided.is_some() {
        conditions.push(format!("m.evidence_provided = ${}", bind_index));
        bind_index += 1;
    }

    if params.project_id.is_some() {
        conditions.push(format!("vc.project_id = ${}", bind_index));
        bind_index += 1;
    }

    // KI: time-range filter on milestones, matching whichever of complete_time
    // or withdraw_time is set on the row.
    if params.from_time.is_some() {
        conditions.push(format!(
            "(m.complete_time >= ${0} OR m.withdraw_time >= ${0})",
            bind_index
        ));
        bind_index += 1;
    }

    if params.to_time.is_some() {
        conditions.push(format!(
            "(m.complete_time <= ${0} OR m.withdraw_time <= ${0})",
            bind_index
        ));
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
        Some("withdraw_time") => "m.withdraw_time DESC NULLS LAST",
        Some("amount") => "m.amount_lovelace DESC NULLS LAST",
        _ => "vc.project_id, m.milestone_order",
    };

    // Get total count
    let count_query = format!(
        r#"
        SELECT COUNT(*)
        FROM treasury.milestones m
        JOIN treasury.projects vc ON vc.id = m.project_db_id
        {}
        "#,
        where_clause
    );

    let mut count_q = sqlx::query_as::<_, (i64,)>(&count_query);

    if let Some(archived) = params.archived {
        count_q = count_q.bind(archived);
    }
    if let Some(withdrawn) = params.withdrawn {
        count_q = count_q.bind(withdrawn);
    }
    if let Some(evidence_provided) = params.evidence_provided {
        count_q = count_q.bind(evidence_provided);
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

    let (total_count,) = count_q.fetch_one(&pool).await?;

    // Get data
    let data_query = format!(
        r#"
        SELECT
            m.id,
            m.project_db_id,
            m.milestone_id,
            m.milestone_order,
            m.label,
            m.description,
            m.acceptance_criteria,
            m.amount_lovelace,
            m.time_limit,
            m.withdrawn,
            m.evidence_provided,
            m.paused,
            m.archived,
            m.complete_tx_hash,
            m.complete_time,
            m.complete_description,
            m.evidence,
            m.withdraw_tx_hash,
            m.withdraw_time,
            m.withdraw_amount,
            m.archived_by_tx_hash,
            m.archived_at,
            m.superseded_by,
            vc.project_id,
            vc.project_name
        FROM treasury.milestones m
        JOIN treasury.projects vc ON vc.id = m.project_db_id
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

    if let Some(archived) = params.archived {
        data_q = data_q.bind(archived);
    }
    if let Some(withdrawn) = params.withdrawn {
        data_q = data_q.bind(withdrawn);
    }
    if let Some(evidence_provided) = params.evidence_provided {
        data_q = data_q.bind(evidence_provided);
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

    let rows = data_q.bind(limit_i64).bind(offset).fetch_all(&pool).await?;

    let milestones: Vec<MilestoneResponse> = rows.into_iter().map(MilestoneResponse::from).collect();
    Ok(Json(PaginatedResponse::new(milestones, page, limit, total_count)))
}

/// Get a specific milestone by integer database ID
///
/// Returns detailed information about a specific milestone. The integer
/// database ID is rarely useful to clients; prefer
/// `/api/v1/milestones/{project_id}` for the human-readable project-scoped
/// list.
#[utoipa::path(
    get,
    path = "/api/v1/milestones/by-id/{id}",
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
) -> Result<Json<ApiResponse<MilestoneResponse>>, ApiError> {
    let row = sqlx::query_as::<_, MilestoneRow>(
        r#"
        SELECT
            m.id,
            m.project_db_id,
            m.milestone_id,
            m.milestone_order,
            m.label,
            m.description,
            m.acceptance_criteria,
            m.amount_lovelace,
            m.time_limit,
            m.withdrawn,
            m.evidence_provided,
            m.paused,
            m.archived,
            m.complete_tx_hash,
            m.complete_time,
            m.complete_description,
            m.evidence,
            m.withdraw_tx_hash,
            m.withdraw_time,
            m.withdraw_amount,
            m.archived_by_tx_hash,
            m.archived_at,
            m.superseded_by,
            vc.project_id,
            vc.project_name
        FROM treasury.milestones m
        JOIN treasury.projects vc ON vc.id = m.project_db_id
        WHERE m.id = $1
        "#
    )
    .bind(id)
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| ApiError::NotFound(format!("milestone `{}` not found", id)))?;

    Ok(Json(ApiResponse::new(MilestoneResponse::from(row))))
}

/// List milestones for a project (paginated)
///
/// Convenience endpoint mirroring `/api/v1/projects/{project_id}/milestones`,
/// served under `/api/v1/milestones/{project_id}` for clients that prefer
/// the milestones-rooted hierarchy.
#[utoipa::path(
    get,
    path = "/api/v1/milestones/{project_id}",
    params(
        ("project_id" = String, Path, description = "Project identifier (e.g., EC-0008-25)"),
        PaginationQuery
    ),
    responses(
        (status = 200, description = "Milestones for the project", body = PaginatedResponse<Vec<MilestoneResponse>>),
        (status = 404, description = "Project not found", body = crate::errors::ApiErrorBody)
    ),
    tag = "Milestones"
)]
pub async fn list_milestones_by_project(
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
            "project `{}` not found",
            project_id
        )));
    }

    let (total_count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM treasury.milestones m
        JOIN treasury.projects p ON p.id = m.project_db_id
        WHERE p.project_id = $1 AND NOT m.archived
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
            p.project_id, p.project_name
        FROM treasury.milestones m
        JOIN treasury.projects p ON p.id = m.project_db_id
        WHERE p.project_id = $1 AND NOT m.archived
        ORDER BY m.milestone_order
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&project_id)
    .bind(limit_i64)
    .bind(offset)
    .fetch_all(&pool)
    .await?;

    let milestones: Vec<MilestoneResponse> =
        rows.into_iter().map(MilestoneResponse::from).collect();
    Ok(Json(PaginatedResponse::new(milestones, page, limit, total_count)))
}
