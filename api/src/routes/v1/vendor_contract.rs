//! Vendor Contract endpoint (singleton — the shared PSSC)

use axum::{extract::Extension, response::Json};
use sqlx::PgPool;
use std::collections::HashMap;

use crate::errors::ApiError;
use crate::models::v1::{
    ApiResponse, VendorContractProjectsBlock, VendorContractResponse,
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
