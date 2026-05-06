//! Vendor Contract endpoint (singleton — the shared PSSC)

use axum::{
    extract::{Extension, Query},
    response::Json,
};
use sqlx::PgPool;
use std::collections::HashMap;

use crate::errors::ApiError;
use crate::models::v1::{
    ApiResponse, PaginatedResponse, PaginationQuery, ProjectUtxoResponse, ProjectUtxoRow,
    VendorContractProjectsBlock, VendorContractResponse,
};

/// Get the shared vendor contract (PSSC) details
///
/// Returns the singleton vendor contract — the on-chain script address
/// where every project's funds sit, distinguished only by inline datum —
/// plus a quick rollup of the projects bound to it.
#[utoipa::path(
    get,
    path = "/api/v1/vendor-contract",
    responses(
        (status = 200, description = "Vendor contract details", body = ApiResponse<VendorContractResponse>),
        (status = 404, description = "Vendor contract not yet known", body = crate::errors::ApiErrorBody)
    ),
    tag = "Vendor Contract"
)]
pub async fn get_vendor_contract(
    Extension(pool): Extension<PgPool>,
) -> Result<Json<ApiResponse<VendorContractResponse>>, ApiError> {
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT address, stake_credential FROM treasury.vendor_contracts ORDER BY id LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?;

    let (address, stake_credential) = row.ok_or_else(|| {
        ApiError::NotFound(
            "vendor contract not yet known — first fund event has not been processed"
                .into(),
        )
    })?;

    let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM treasury.projects")
        .fetch_one(&pool)
        .await?;

    let by_status_rows: Vec<(Option<String>, i64)> = sqlx::query_as(
        "SELECT status, COUNT(*) FROM treasury.projects GROUP BY status",
    )
    .fetch_all(&pool)
    .await?;

    let by_status: HashMap<String, i64> = by_status_rows
        .into_iter()
        .map(|(status, count)| (status.unwrap_or_else(|| "unknown".into()), count))
        .collect();

    Ok(Json(ApiResponse::new(VendorContractResponse {
        address,
        stake_credential,
        projects: VendorContractProjectsBlock { total, by_status },
    })))
}

/// Get currently-unspent UTxOs at the shared vendor contract, labeled per project.
///
/// Returns every live output sitting at the singleton PSSC, with each row
/// carrying its owning project's `project_id`, `project_name`, and `project_status`
/// so callers can enumerate vendor-contract state in a single round trip
/// instead of fanning out across every project.
///
/// "Currently unspent" is sourced from `yaci_store.address_utxo` with an
/// anti-join against `yaci_store.tx_input` to ride out the trigger
/// pruning-window lag (same approach used by `/projects/:id/utxos` and
/// `/treasury/utxos`).
#[utoipa::path(
    get,
    path = "/api/v1/vendor-contract/utxos",
    params(PaginationQuery),
    responses(
        (status = 200, description = "Currently-unspent UTxOs at the vendor contract, labeled per project",
         body = PaginatedResponse<Vec<ProjectUtxoResponse>>),
        (status = 404, description = "Vendor contract not yet known", body = crate::errors::ApiErrorBody)
    ),
    tag = "Vendor Contract"
)]
pub async fn get_vendor_contract_utxos(
    Extension(pool): Extension<PgPool>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<PaginatedResponse<Vec<ProjectUtxoResponse>>>, ApiError> {
    let page = params.page.max(1);
    let limit = params.limit.min(100).max(1);
    let offset = ((page - 1) * limit) as i64;
    let limit_i64 = limit as i64;

    let address: String = sqlx::query_scalar(
        "SELECT address FROM treasury.vendor_contracts ORDER BY id LIMIT 1",
    )
    .fetch_optional(&pool)
    .await?
    .ok_or_else(|| {
        ApiError::NotFound(
            "vendor contract not yet known — first fund event has not been processed".into(),
        )
    })?;

    let (total_count,): (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM yaci_store.address_utxo au
        JOIN treasury.utxo_history uh
            ON uh.tx_hash = au.tx_hash AND uh.output_index = au.output_index
        JOIN treasury.projects p ON p.id = uh.project_db_id
        WHERE au.owner_addr = $1
          AND NOT EXISTS (
              SELECT 1 FROM yaci_store.tx_input ti
              WHERE ti.tx_hash = au.tx_hash AND ti.output_index = au.output_index
          )
        "#,
    )
    .bind(&address)
    .fetch_one(&pool)
    .await?;

    let rows = sqlx::query_as::<_, ProjectUtxoRow>(
        r#"
        SELECT
            au.tx_hash,
            au.output_index,
            au.owner_addr   AS address,
            au.lovelace_amount,
            au.slot,
            au.block        AS block_number,
            p.id            AS project_db_id,
            p.project_id    AS project_id,
            p.project_name  AS project_name,
            p.status        AS project_status
        FROM yaci_store.address_utxo au
        JOIN treasury.utxo_history uh
            ON uh.tx_hash = au.tx_hash AND uh.output_index = au.output_index
        JOIN treasury.projects p ON p.id = uh.project_db_id
        WHERE au.owner_addr = $1
          AND NOT EXISTS (
              SELECT 1 FROM yaci_store.tx_input ti
              WHERE ti.tx_hash = au.tx_hash AND ti.output_index = au.output_index
          )
        ORDER BY au.slot DESC, au.tx_hash ASC, au.output_index ASC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(&address)
    .bind(limit_i64)
    .bind(offset)
    .fetch_all(&pool)
    .await?;

    let utxos: Vec<ProjectUtxoResponse> =
        rows.into_iter().map(ProjectUtxoResponse::from).collect();
    Ok(Json(PaginatedResponse::new(utxos, page, limit, total_count)))
}
