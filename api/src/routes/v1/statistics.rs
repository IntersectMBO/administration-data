//! Statistics endpoint

use axum::{extract::Extension, response::Json};
use sqlx::PgPool;
use std::collections::HashMap;

use crate::errors::ApiError;
use crate::models::v1::{
    ApiResponse, EventStats, FinancialStats, MilestoneStats, ProjectStats,
    StatisticsResponse, SyncStats, TreasuryStats, VendorContractStats,
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
) -> Result<Json<ApiResponse<StatisticsResponse>>, ApiError> {
    let treasury_stats = get_treasury_stats(&pool).await?;
    let vendor_contract_stats = get_vendor_contract_stats(&pool).await?;
    let project_stats = get_project_stats(&pool).await?;
    let milestone_stats = get_milestone_stats(&pool).await?;
    let event_stats = get_event_stats(&pool).await?;
    let financial_stats = get_financial_stats(&pool).await?;
    let sync_stats = get_sync_stats(&pool).await?;

    Ok(Json(ApiResponse::new(StatisticsResponse {
        treasury: treasury_stats,
        vendor_contracts: vendor_contract_stats,
        projects: project_stats,
        milestones: milestone_stats,
        events: event_stats,
        financials: financial_stats,
        sync: sync_stats,
    })))
}

async fn get_vendor_contract_stats(pool: &PgPool) -> Result<VendorContractStats, ApiError> {
    let (total_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM treasury.vendor_contracts")
            .fetch_one(pool)
            .await?;

    let address: Option<String> =
        sqlx::query_scalar("SELECT address FROM treasury.vendor_contracts ORDER BY id LIMIT 1")
            .fetch_optional(pool)
            .await?;

    let (project_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM treasury.projects")
        .fetch_one(pool)
        .await?;

    // utxo_history_count is the lifetime capture count (script-address only).
    // unspent_utxo_count + current_balance_lovelace come from yaci's live UTXO set
    // (authoritative — pruned spent rows can't leak in, ghost-unspent rows in
    // utxo_history are excluded by the JOIN).
    // utxo_history_count is the lifetime capture count (script-address only).
    // unspent_utxo_count + current_balance_lovelace come from yaci's live UTXO set.
    // yaci_store.address_utxo has no `spent` column — pruning removes rows. The
    // anti-join against tx_input handles the pruning-window lag.
    let (utxo_history_count, unspent_utxo_count, current_balance_lovelace): (i64, i64, Option<i64>) =
        if let Some(ref addr) = address {
            sqlx::query_as(
                r#"
                SELECT
                    (SELECT COUNT(*) FROM treasury.utxo_history WHERE address = $1)::BIGINT,
                    (SELECT COUNT(*) FROM yaci_store.address_utxo au
                       WHERE au.owner_addr = $1
                         AND NOT EXISTS (
                             SELECT 1 FROM yaci_store.tx_input ti
                             WHERE ti.tx_hash = au.tx_hash AND ti.output_index = au.output_index
                         ))::BIGINT,
                    (SELECT COALESCE(SUM(au.lovelace_amount), 0) FROM yaci_store.address_utxo au
                       WHERE au.owner_addr = $1
                         AND NOT EXISTS (
                             SELECT 1 FROM yaci_store.tx_input ti
                             WHERE ti.tx_hash = au.tx_hash AND ti.output_index = au.output_index
                         ))::BIGINT
                "#,
            )
            .bind(addr)
            .fetch_one(pool)
            .await?
        } else {
            (0, 0, Some(0))
        };

    Ok(VendorContractStats {
        total_count,
        address,
        project_count,
        utxo_history_count,
        unspent_utxo_count,
        current_balance_lovelace: current_balance_lovelace.unwrap_or(0),
    })
}

async fn get_treasury_stats(pool: &PgPool) -> Result<TreasuryStats, ApiError> {
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
    ?;

    // Get disbursement count from events
    let (disbursed_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM treasury.events WHERE event_type = 'disburse'"
    )
    .fetch_one(pool)
    .await
    ?;

    Ok(TreasuryStats {
        total_count: row.0,
        active_count: row.1,
        disbursed_count,
    })
}

async fn get_project_stats(pool: &PgPool) -> Result<ProjectStats, ApiError> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
        r#"
        SELECT
            COUNT(*),
            COUNT(*) FILTER (WHERE status = 'active'),
            COUNT(*) FILTER (WHERE status = 'completed'),
            COUNT(*) FILTER (WHERE status = 'paused'),
            COUNT(*) FILTER (WHERE status = 'cancelled')
        FROM treasury.projects
        "#
    )
    .fetch_one(pool)
    .await
    ?;

    Ok(ProjectStats {
        total_count: row.0,
        active_count: row.1,
        completed_count: row.2,
        paused_count: row.3,
        cancelled_count: row.4,
    })
}

async fn get_milestone_stats(pool: &PgPool) -> Result<MilestoneStats, ApiError> {
    let row = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE NOT archived),
            COUNT(*) FILTER (WHERE NOT archived AND NOT evidence_provided AND NOT withdrawn),
            COUNT(*) FILTER (WHERE NOT archived AND evidence_provided AND NOT withdrawn),
            COUNT(*) FILTER (WHERE NOT archived AND withdrawn)
        FROM treasury.milestones
        "#
    )
    .fetch_one(pool)
    .await
    ?;

    Ok(MilestoneStats {
        total_count: row.0,
        pending_count: row.1,
        completed_count: row.2,
        withdrawn_count: row.3,
    })
}

async fn get_event_stats(pool: &PgPool) -> Result<EventStats, ApiError> {
    // Get processed event count
    let (processed_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM treasury.events")
        .fetch_one(pool)
        .await
        ?;

    // Get on-chain TOM event count and breakdown by type from yaci_store.
    // Event type lives at body.body.event in the metadata (see event_processor::process_event).
    let on_chain_rows = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT
            COALESCE(LOWER(body::jsonb -> 'body' ->> 'event'), 'unknown') AS event_type,
            COUNT(*)
        FROM yaci_store.transaction_metadata
        WHERE label = '1694'
        GROUP BY 1
        ORDER BY COUNT(*) DESC
        "#
    )
    .fetch_all(pool)
    .await
    ?;

    let on_chain_count: i64 = on_chain_rows.iter().map(|(_, c)| c).sum();
    let by_type: HashMap<String, i64> = on_chain_rows.into_iter().collect();

    Ok(EventStats {
        on_chain_count,
        processed_count,
        by_type,
    })
}

async fn get_financial_stats(pool: &PgPool) -> Result<FinancialStats, ApiError> {
    // Get total allocated (sum of initial amounts)
    let (total_allocated,): (Option<i64>,) = sqlx::query_as(
        "SELECT COALESCE(SUM(initial_amount_lovelace), 0)::BIGINT FROM treasury.projects"
    )
    .fetch_one(pool)
    .await
    ?;

    // Get total withdrawn
    let (total_withdrawn,): (Option<i64>,) = sqlx::query_as(
        "SELECT COALESCE(SUM(withdraw_amount), 0)::BIGINT FROM treasury.milestones WHERE withdrawn AND NOT archived"
    )
    .fetch_one(pool)
    .await
    ?;

    // Current balance: raw live unspent at TRSC + PSSC addresses, from yaci's UTXO
    // set (rows pruned on spend; anti-join against tx_input handles pruning lag).
    // We do NOT trust treasury.utxo_history.spent — pre-trigger captures and
    // KI-UTX-02 non-script captures leave stale spent=FALSE rows that would
    // over-count. We do NOT join through utxo_history for project attribution
    // either — chain-trace gaps would *under*-count the vendor contract balance.
    let (current_balance,): (Option<i64>,) = sqlx::query_as(
        r#"
        SELECT (
            COALESCE((
                SELECT SUM(au.lovelace_amount)
                FROM yaci_store.address_utxo au
                JOIN treasury.treasury_contracts tc ON tc.contract_address = au.owner_addr
                WHERE NOT EXISTS (
                    SELECT 1 FROM yaci_store.tx_input ti
                    WHERE ti.tx_hash = au.tx_hash AND ti.output_index = au.output_index
                )
            ), 0)
            +
            COALESCE((
                SELECT SUM(au.lovelace_amount)
                FROM yaci_store.address_utxo au
                JOIN treasury.vendor_contracts vco ON vco.address = au.owner_addr
                WHERE NOT EXISTS (
                    SELECT 1 FROM yaci_store.tx_input ti
                    WHERE ti.tx_hash = au.tx_hash AND ti.output_index = au.output_index
                )
            ), 0)
        )::BIGINT
        "#
    )
    .fetch_one(pool)
    .await
    ?;

    let allocated = total_allocated.unwrap_or(0);
    let withdrawn = total_withdrawn.unwrap_or(0);
    let balance = current_balance.unwrap_or(0);

    Ok(FinancialStats {
        total_allocated_lovelace: allocated,
        total_withdrawn_lovelace: withdrawn,
        current_balance_lovelace: balance,
    })
}

async fn get_sync_stats(pool: &PgPool) -> Result<SyncStats, ApiError> {
    let row = sqlx::query_as::<_, (Option<i64>, Option<i64>, Option<chrono::DateTime<chrono::Utc>>)>(
        "SELECT last_slot, last_block, updated_at FROM treasury.sync_status WHERE sync_type = 'events'"
    )
    .fetch_optional(pool)
    .await
    ?;

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
