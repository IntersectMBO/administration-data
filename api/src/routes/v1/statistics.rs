//! Statistics endpoint

use axum::{extract::Extension, http::StatusCode, response::Json};
use sqlx::PgPool;
use std::collections::HashMap;

use crate::models::v1::{
    lovelace_to_ada, ApiResponse, EventStats, FinancialStats, MilestoneStats, ProjectStats,
    StatisticsResponse, SyncStats, TreasuryStats,
};

/// Get comprehensive statistics
///
/// Returns aggregated statistics across treasury, projects, milestones, events, and financials.
#[utoipa::path(
    get,
    path = "/api/v1/statistics",
    responses(
        (status = 200, description = "Comprehensive statistics", body = ApiResponse<StatisticsResponse>)
    ),
    tag = "Statistics"
)]
pub async fn get_statistics(
    Extension(pool): Extension<PgPool>,
) -> Result<Json<ApiResponse<StatisticsResponse>>, StatusCode> {
    // Treasury stats
    let treasury_stats = get_treasury_stats(&pool).await?;

    // Project stats
    let project_stats = get_project_stats(&pool).await?;

    // Milestone stats
    let milestone_stats = get_milestone_stats(&pool).await?;

    // Event stats
    let event_stats = get_event_stats(&pool).await?;

    // Financial stats
    let financial_stats = get_financial_stats(&pool).await?;

    // Sync stats
    let sync_stats = get_sync_stats(&pool).await?;

    Ok(Json(ApiResponse::new(StatisticsResponse {
        treasury: treasury_stats,
        projects: project_stats,
        milestones: milestone_stats,
        events: event_stats,
        financials: financial_stats,
        sync: sync_stats,
    })))
}

async fn get_treasury_stats(pool: &PgPool) -> Result<TreasuryStats, StatusCode> {
    let row = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT
            COUNT(*),
            COUNT(*) FILTER (WHERE status = 'active')
        FROM treasury.treasury_contracts
        "#
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Get disbursement count from events
    let (disbursed_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM treasury.events WHERE event_type = 'disburse'"
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(TreasuryStats {
        total_count: row.0,
        active_count: row.1,
        disbursed_count,
    })
}

async fn get_project_stats(pool: &PgPool) -> Result<ProjectStats, StatusCode> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        r#"
        SELECT
            COUNT(*),
            COUNT(*) FILTER (WHERE status = 'active'),
            COUNT(*) FILTER (WHERE status = 'completed'),
            COUNT(*) FILTER (WHERE status = 'paused'),
            COUNT(*) FILTER (WHERE status = 'cancelled')
        FROM treasury.vendor_contracts
        "#
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(ProjectStats {
        total_count: row.0,
        active_count: row.1,
        completed_count: row.2,
        paused_count: row.3,
        cancelled_count: row.4,
    })
}

async fn get_milestone_stats(pool: &PgPool) -> Result<MilestoneStats, StatusCode> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        r#"
        SELECT
            COUNT(*),
            COUNT(*) FILTER (WHERE status = 'pending'),
            COUNT(*) FILTER (WHERE status = 'completed'),
            COUNT(*) FILTER (WHERE status = 'withdrawn')
        FROM treasury.milestones
        "#
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(MilestoneStats {
        total_count: row.0,
        pending_count: row.1,
        completed_count: row.2,
        withdrawn_count: row.3,
    })
}

async fn get_event_stats(pool: &PgPool) -> Result<EventStats, StatusCode> {
    // Get total count
    let (total_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM treasury.events")
        .fetch_one(pool)
        .await
        .map_err(|e| {
            tracing::error!("Database query error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Get counts by type
    let type_rows = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT event_type, COUNT(*)
        FROM treasury.events
        GROUP BY event_type
        ORDER BY COUNT(*) DESC
        "#
    )
    .fetch_all(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let by_type: HashMap<String, i64> = type_rows.into_iter().collect();

    Ok(EventStats {
        total_count,
        by_type,
    })
}

async fn get_financial_stats(pool: &PgPool) -> Result<FinancialStats, StatusCode> {
    // Get total allocated (sum of initial amounts)
    let (total_allocated,): (Option<i64>,) = sqlx::query_as(
        "SELECT COALESCE(SUM(initial_amount_lovelace), 0)::BIGINT FROM treasury.vendor_contracts"
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Get total disbursed
    let (total_disbursed,): (Option<i64>,) = sqlx::query_as(
        "SELECT COALESCE(SUM(disburse_amount), 0)::BIGINT FROM treasury.milestones WHERE status = 'disbursed'"
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Get current balance (unspent UTXOs)
    let (current_balance,): (Option<i64>,) = sqlx::query_as(
        "SELECT COALESCE(SUM(lovelace_amount), 0)::BIGINT FROM treasury.utxos WHERE NOT spent"
    )
    .fetch_one(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let allocated = total_allocated.unwrap_or(0);
    let disbursed = total_disbursed.unwrap_or(0);
    let balance = current_balance.unwrap_or(0);

    Ok(FinancialStats {
        total_allocated_lovelace: allocated,
        total_allocated_ada: lovelace_to_ada(allocated),
        total_disbursed_lovelace: disbursed,
        total_disbursed_ada: lovelace_to_ada(disbursed),
        current_balance_lovelace: balance,
        current_balance_ada: lovelace_to_ada(balance),
    })
}

async fn get_sync_stats(pool: &PgPool) -> Result<SyncStats, StatusCode> {
    let row = sqlx::query_as::<_, (Option<i64>, Option<i64>, Option<chrono::DateTime<chrono::Utc>>)>(
        "SELECT last_slot, last_block, updated_at FROM treasury.sync_status WHERE sync_type = 'events'"
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        tracing::error!("Database query error: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    match row {
        Some((last_slot, last_block, updated_at)) => Ok(SyncStats {
            last_slot,
            last_block,
            last_updated: updated_at,
        }),
        None => Ok(SyncStats {
            last_slot: None,
            last_block: None,
            last_updated: None,
        }),
    }
}
