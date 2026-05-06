//! V1 API Models with OpenAPI support
//!
//! These models follow the new API design with:
//! - Amounts in lovelace (1 ADA = 1,000,000 lovelace) — single source of truth
//! - On-chain block times paired as `{unix, iso}` via [`crate::models::time::ChainTime`]
//! - Raw metadata AND parsed/normalized data
//! - Consistent response envelopes with pagination
//! - Structured error envelope via [`crate::errors::ApiErrorBody`]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::{IntoParams, ToSchema};

use crate::models::time::ChainTime;

// ============================================================================
// RESPONSE ENVELOPE
// ============================================================================

/// Standard API response envelope
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiResponse<T> {
    /// The response data
    pub data: T,
    /// Response metadata
    pub meta: ResponseMeta,
}

/// Standard API response envelope with pagination
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PaginatedResponse<T> {
    /// The response data
    pub data: T,
    /// Pagination information
    pub pagination: Pagination,
    /// Response metadata
    pub meta: ResponseMeta,
}

/// Pagination information
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct Pagination {
    /// Current page number (1-indexed)
    pub page: u32,
    /// Items per page
    pub limit: u32,
    /// Total number of items
    pub total_count: i64,
    /// Whether there are more pages
    pub has_next: bool,
}

impl Pagination {
    pub fn new(page: u32, limit: u32, total_count: i64) -> Self {
        let has_next = (page as i64 * limit as i64) < total_count;
        Self {
            page,
            limit,
            total_count,
            has_next,
        }
    }
}

/// Response metadata
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ResponseMeta {
    /// When this response was generated
    pub timestamp: DateTime<Utc>,
}

impl Default for ResponseMeta {
    fn default() -> Self {
        Self {
            timestamp: Utc::now(),
        }
    }
}

// Helper functions to create responses
impl<T> ApiResponse<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            meta: ResponseMeta::default(),
        }
    }
}

impl<T> PaginatedResponse<T> {
    pub fn new(data: T, page: u32, limit: u32, total_count: i64) -> Self {
        Self {
            data,
            pagination: Pagination::new(page, limit, total_count),
            meta: ResponseMeta::default(),
        }
    }
}

// ============================================================================
// STATUS & HEALTH
// ============================================================================

/// API status response.
///
/// Three time domains are surfaced separately:
///
/// - `database.checked_at` — server-side; when the response was generated.
/// - `sync.heartbeat` — server-side; last time the API's TOM-sync loop ran.
///   Bumps every poll regardless of whether new events arrived.
/// - `sync.last_event_processed` — on-chain; block time of the most recent
///   TOM event the API has written into `treasury.events`.
/// - `chain.indexer_time` — on-chain; block time of the most recent block
///   YACI Store has ingested. Tells you whether YACI is at tip.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StatusResponse {
    /// API version
    pub api_version: String,
    /// Database health
    pub database: DatabaseStatus,
    /// API-side sync state
    pub sync: SyncStatusBlock,
    /// On-chain indexer state
    pub chain: ChainStatus,
    /// Top-level counts
    pub totals: TotalsBlock,
}

/// Database health subsection
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DatabaseStatus {
    /// Whether the API can talk to Postgres
    pub connected: bool,
    /// When this status response was generated (server-side, ISO 8601)
    pub checked_at: DateTime<Utc>,
}

/// API-side sync subsection
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncStatusBlock {
    /// Server-side timestamp of the last TOM-sync poll. Bumps every 15 s
    /// regardless of whether new events arrived (KI-SY-01).
    pub heartbeat: Option<DateTime<Utc>>,
    /// On-chain block time of the most recent TOM event the API has
    /// processed into `treasury.events`. Null if no events processed yet.
    pub last_event_processed: Option<ChainTime>,
}

/// On-chain indexer subsection
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ChainStatus {
    /// Block number the YACI Store indexer has reached (most recent block).
    pub indexer_block: Option<i64>,
    /// Slot the indexer has reached.
    pub indexer_slot: Option<i64>,
    /// On-chain block time of the indexer's most recent block.
    pub indexer_time: Option<ChainTime>,
}

/// Top-level row counts
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TotalsBlock {
    pub events: i64,
    pub projects: i64,
    /// Count of `treasury.events` rows by `event_type`.
    pub events_by_type: std::collections::HashMap<String, i64>,
}

// ============================================================================
// VENDOR CONTRACT (singleton — the shared PSSC)
// ============================================================================

/// Response for `/api/v1/vendor-contract` — the *one* shared PSSC script
/// address every project sits at, plus a quick rollup of the projects.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VendorContractResponse {
    /// Shared PSSC script address (`addr1x...`).
    pub address: String,
    /// Stake credential portion of the address.
    pub stake_credential: Option<String>,
    /// Project rollup at this vendor contract.
    pub projects: VendorContractProjectsBlock,
}

/// Project rollup nested inside `VendorContractResponse`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VendorContractProjectsBlock {
    /// Total projects.
    pub total: i64,
    /// Counts keyed by `status` (`active`, `paused`, `completed`, `cancelled`).
    pub by_status: std::collections::HashMap<String, i64>,
}

// ============================================================================
// TREASURY
// ============================================================================

/// Treasury contract details with statistics
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TreasuryResponse {
    /// Internal database ID
    pub id: i32,
    /// On-chain contract instance identifier (policy ID)
    pub contract_instance: String,
    /// Script address
    pub contract_address: Option<String>,
    /// Stake credential
    pub stake_credential: Option<String>,
    /// Contract status (active/paused)
    pub status: Option<String>,
    /// Publish transaction hash
    pub publish_tx_hash: Option<String>,
    /// On-chain publish time (`{unix, iso}`)
    pub publish_time: Option<ChainTime>,
    /// Initialize transaction hash
    pub initialized_tx_hash: Option<String>,
    /// On-chain initialize time (`{unix, iso}`)
    pub initialized_at: Option<ChainTime>,
    /// Permission rules
    pub permissions: Option<serde_json::Value>,
    /// Statistics
    pub statistics: TreasuryStatistics,
    /// Financial summary
    pub financials: TreasuryFinancials,
    /// Record created at
    pub created_at: Option<DateTime<Utc>>,
    /// Record updated at
    pub updated_at: Option<DateTime<Utc>>,
}

/// Treasury statistics
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TreasuryStatistics {
    /// Total projects
    pub project_count: i64,
    /// Active projects
    pub active_contracts: i64,
    /// Completed projects
    pub completed_contracts: i64,
    /// Cancelled projects
    pub cancelled_contracts: i64,
    /// Total events
    pub total_events: i64,
    /// Current UTXO count
    pub utxo_count: i64,
    /// Last event time (`{unix, iso}`)
    pub last_event_time: Option<ChainTime>,
}

/// Treasury financial summary
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TreasuryFinancials {
    /// Treasury balance in lovelace
    pub balance_lovelace: i64,
}

/// Database row for treasury summary
#[derive(Debug, FromRow)]
pub struct TreasurySummaryRow {
    pub treasury_id: i32,
    pub contract_instance: String,
    pub contract_address: Option<String>,
    pub stake_credential: Option<String>,
    pub status: Option<String>,
    pub publish_tx_hash: Option<String>,
    pub publish_time: Option<i64>,
    pub initialized_tx_hash: Option<String>,
    pub initialized_at: Option<i64>,
    pub permissions: Option<serde_json::Value>,
    pub project_count: Option<i64>,
    pub active_contracts: Option<i64>,
    pub completed_contracts: Option<i64>,
    pub cancelled_contracts: Option<i64>,
    pub treasury_balance: Option<i64>,
    pub utxo_count: Option<i64>,
    pub total_events: Option<i64>,
    pub last_event_time: Option<i64>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl From<TreasurySummaryRow> for TreasuryResponse {
    fn from(row: TreasurySummaryRow) -> Self {
        let balance = row.treasury_balance.unwrap_or(0);
        Self {
            id: row.treasury_id,
            contract_instance: row.contract_instance,
            contract_address: row.contract_address,
            stake_credential: row.stake_credential,
            status: row.status,
            publish_tx_hash: row.publish_tx_hash,
            publish_time: ChainTime::maybe_from_secs(row.publish_time),
            initialized_tx_hash: row.initialized_tx_hash,
            initialized_at: ChainTime::maybe_from_secs(row.initialized_at),
            permissions: row.permissions,
            statistics: TreasuryStatistics {
                project_count: row.project_count.unwrap_or(0),
                active_contracts: row.active_contracts.unwrap_or(0),
                completed_contracts: row.completed_contracts.unwrap_or(0),
                cancelled_contracts: row.cancelled_contracts.unwrap_or(0),
                total_events: row.total_events.unwrap_or(0),
                utxo_count: row.utxo_count.unwrap_or(0),
                last_event_time: ChainTime::maybe_from_secs(row.last_event_time),
            },
            financials: TreasuryFinancials {
                balance_lovelace: balance,
            },
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

// ============================================================================
// VENDOR CONTRACTS
// ============================================================================

/// Vendor contract (project) summary
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProjectSummary {
    /// Internal database ID
    pub id: i32,
    /// Logical project identifier (e.g., "EC-0008-25")
    pub project_id: String,
    /// Project name/label
    pub project_name: Option<String>,
    /// Project description
    pub description: Option<String>,
    /// Vendor payment address
    pub vendor_address: Option<String>,
    /// PSSC script address
    pub contract_address: Option<String>,
    /// Contract status (active/paused/completed/cancelled)
    pub status: Option<String>,
    /// Fund transaction hash
    pub fund_tx_hash: String,
    /// On-chain fund time (`{unix, iso}`)
    pub fund_time: Option<ChainTime>,
    /// Initial allocated amount in lovelace
    pub initial_amount_lovelace: Option<i64>,
    /// Milestone summary
    pub milestones_summary: MilestonesSummary,
    /// Financial summary
    pub financials: VendorFinancials,
    /// Treasury reference
    pub treasury: TreasuryReference,
    /// Last event time (`{unix, iso}`)
    pub last_event_time: Option<ChainTime>,
    /// Total event count
    pub event_count: Option<i64>,
}

/// Vendor contract detail (full response)
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProjectDetail {
    /// Internal database ID
    pub id: i32,
    /// Logical project identifier (e.g., "EC-0008-25")
    pub project_id: String,
    /// Other related identifiers
    pub other_identifiers: Option<Vec<String>>,
    /// Project name/label
    pub project_name: Option<String>,
    /// Project description
    pub description: Option<String>,
    /// Vendor payment address
    pub vendor_address: Option<String>,
    /// Vendor payment key hash from datum
    pub vendor_payment_key_hash: Option<String>,
    /// PSSC script address
    pub contract_address: Option<String>,
    /// Contract status (active/paused/completed/cancelled)
    pub status: Option<String>,
    /// Fund transaction hash
    pub fund_tx_hash: String,
    /// On-chain fund time (`{unix, iso}`)
    pub fund_time: Option<ChainTime>,
    /// Initial allocated amount in lovelace
    pub initial_amount_lovelace: Option<i64>,
    /// Milestone summary
    pub milestones_summary: MilestonesSummary,
    /// Financial summary
    pub financials: VendorFinancials,
    /// Treasury reference
    pub treasury: TreasuryReference,
    /// Last event time (`{unix, iso}`)
    pub last_event_time: Option<ChainTime>,
    /// Total event count
    pub event_count: Option<i64>,
    /// Currently-unspent UTxOs belonging to this project at the vendor contract.
    /// Empty when the project has no live outputs (fully withdrawn or pre-fund).
    pub current_utxos: Vec<ProjectCurrentUtxo>,
    /// Record created at
    pub created_at: Option<DateTime<Utc>>,
    /// Record updated at
    pub updated_at: Option<DateTime<Utc>>,
}

/// Milestones summary counts
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MilestonesSummary {
    /// Total milestones
    pub total: i64,
    /// Pending milestones
    pub pending: i64,
    /// Completed milestones (evidence provided but not yet withdrawn)
    pub completed: i64,
    /// Withdrawn milestones
    pub withdrawn: i64,
    /// Paused milestones
    pub paused: i64,
}

/// Vendor contract financial summary
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VendorFinancials {
    /// Total allocated amount in lovelace
    pub total_allocated_lovelace: i64,
    /// Total withdrawn amount in lovelace
    pub total_withdrawn_lovelace: i64,
    /// Current balance in lovelace (from UTXOs)
    pub current_balance_lovelace: i64,
    /// Withdrawal percentage
    pub withdrawal_percentage: f64,
    /// UTXO count
    pub utxo_count: i64,
}

/// Treasury reference
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TreasuryReference {
    /// Contract instance identifier
    pub contract_instance: Option<String>,
}

/// Compact UTxO reference embedded on `ProjectDetail` to give clients the
/// project's currently-unspent outputs without a second round trip.
#[derive(Debug, Serialize, Deserialize, ToSchema, FromRow)]
pub struct ProjectCurrentUtxo {
    /// Transaction hash
    pub tx_hash: String,
    /// Output index
    pub output_index: i16,
    /// Amount in lovelace
    pub lovelace_amount: Option<i64>,
    /// Creation slot
    pub slot: Option<i64>,
}

/// Database row for vendor contract summary
#[derive(Debug, FromRow)]
#[allow(dead_code)]
pub struct ProjectSummaryRow {
    pub id: i32,
    pub treasury_id: Option<i32>,
    pub project_id: String,
    pub other_identifiers: Option<Vec<String>>,
    pub project_name: Option<String>,
    pub description: Option<String>,
    pub vendor_address: Option<String>,
    pub contract_address: Option<String>,
    pub fund_tx_hash: String,
    pub fund_slot: Option<i64>,
    pub fund_block_time: Option<i64>,
    pub initial_amount_lovelace: Option<i64>,
    pub status: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub treasury_instance: Option<String>,
    pub total_milestones: Option<i64>,
    pub pending_milestones: Option<i64>,
    pub completed_milestones: Option<i64>,
    pub withdrawn_milestones: Option<i64>,
    pub paused_milestones: Option<i64>,
    pub total_withdrawn_lovelace: Option<i64>,
    pub current_balance_lovelace: Option<i64>,
    pub utxo_count: Option<i64>,
    pub last_event_time: Option<i64>,
    pub event_count: Option<i64>,
}

impl From<ProjectSummaryRow> for ProjectSummary {
    fn from(row: ProjectSummaryRow) -> Self {
        let initial_amount = row.initial_amount_lovelace.unwrap_or(0);
        let total_withdrawn = row.total_withdrawn_lovelace.unwrap_or(0);
        let current_balance = row.current_balance_lovelace.unwrap_or(0);
        let withdrawal_pct = if initial_amount > 0 {
            (total_withdrawn as f64 / initial_amount as f64) * 100.0
        } else {
            0.0
        };

        Self {
            id: row.id,
            project_id: row.project_id,
            project_name: row.project_name,
            description: row.description,
            vendor_address: row.vendor_address,
            contract_address: row.contract_address,
            status: row.status,
            fund_tx_hash: row.fund_tx_hash,
            fund_time: ChainTime::maybe_from_secs(row.fund_block_time),
            initial_amount_lovelace: row.initial_amount_lovelace,
            milestones_summary: MilestonesSummary {
                total: row.total_milestones.unwrap_or(0),
                pending: row.pending_milestones.unwrap_or(0),
                completed: row.completed_milestones.unwrap_or(0),
                withdrawn: row.withdrawn_milestones.unwrap_or(0),
                paused: row.paused_milestones.unwrap_or(0),
            },
            financials: VendorFinancials {
                total_allocated_lovelace: initial_amount,
                total_withdrawn_lovelace: total_withdrawn,
                current_balance_lovelace: current_balance,
                withdrawal_percentage: withdrawal_pct,
                utxo_count: row.utxo_count.unwrap_or(0),
            },
            treasury: TreasuryReference {
                contract_instance: row.treasury_instance,
            },
            last_event_time: ChainTime::maybe_from_secs(row.last_event_time),
            event_count: row.event_count,
        }
    }
}

impl From<ProjectSummaryRow> for ProjectDetail {
    fn from(row: ProjectSummaryRow) -> Self {
        let initial_amount = row.initial_amount_lovelace.unwrap_or(0);
        let total_withdrawn = row.total_withdrawn_lovelace.unwrap_or(0);
        let current_balance = row.current_balance_lovelace.unwrap_or(0);
        let withdrawal_pct = if initial_amount > 0 {
            (total_withdrawn as f64 / initial_amount as f64) * 100.0
        } else {
            0.0
        };

        Self {
            id: row.id,
            project_id: row.project_id,
            other_identifiers: row.other_identifiers,
            project_name: row.project_name,
            description: row.description,
            vendor_address: row.vendor_address,
            vendor_payment_key_hash: None, // populated from DB when queried directly
            contract_address: row.contract_address,
            status: row.status,
            fund_tx_hash: row.fund_tx_hash,
            fund_time: ChainTime::maybe_from_secs(row.fund_block_time),
            initial_amount_lovelace: row.initial_amount_lovelace,
            milestones_summary: MilestonesSummary {
                total: row.total_milestones.unwrap_or(0),
                pending: row.pending_milestones.unwrap_or(0),
                completed: row.completed_milestones.unwrap_or(0),
                withdrawn: row.withdrawn_milestones.unwrap_or(0),
                paused: row.paused_milestones.unwrap_or(0),
            },
            financials: VendorFinancials {
                total_allocated_lovelace: initial_amount,
                total_withdrawn_lovelace: total_withdrawn,
                current_balance_lovelace: current_balance,
                withdrawal_percentage: withdrawal_pct,
                utxo_count: row.utxo_count.unwrap_or(0),
            },
            treasury: TreasuryReference {
                contract_instance: row.treasury_instance,
            },
            last_event_time: ChainTime::maybe_from_secs(row.last_event_time),
            event_count: row.event_count,
            current_utxos: Vec::new(), // populated by handler after this conversion
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

// ============================================================================
// MILESTONES
// ============================================================================

/// Milestone response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MilestoneResponse {
    /// Internal database ID
    pub id: i32,
    /// Logical milestone identifier (e.g., "m-0")
    pub milestone_id: String,
    /// Milestone order/position
    pub milestone_order: i32,
    /// Milestone label/name
    pub label: Option<String>,
    /// Milestone description
    pub description: Option<String>,
    /// Acceptance criteria
    pub acceptance_criteria: Option<String>,
    /// Allocated amount in lovelace
    pub amount_lovelace: Option<i64>,
    /// Time limit (POSIXTime in milliseconds)
    pub time_limit: Option<i64>,
    /// Whether the vendor has withdrawn funds
    pub withdrawn: bool,
    /// Whether completion evidence has been provided
    pub evidence_provided: bool,
    /// Whether this milestone is currently paused (latest pause/resume state)
    pub paused: bool,
    /// Whether this milestone has been archived (replaced by a modify event)
    pub archived: bool,
    /// Completion details
    pub completion: Option<MilestoneCompletion>,
    /// Withdrawal details
    pub withdrawal: Option<MilestoneWithdrawal>,
    /// Archive info (present when archived)
    pub archive_info: Option<MilestoneArchiveInfo>,
    /// Pause/resume history (present when at least one pause event has been recorded)
    pub pause_history: Option<MilestonePauseHistory>,
    /// Project reference
    pub project: ProjectReference,
}

/// Pause/resume history for a milestone.
///
/// Present when at least one pause OR resume event has been recorded for the
/// milestone. `currently_paused` reflects the milestone's current state (set
/// by the contract output datum). Either `last_pause_*` or `last_resume_*`
/// may be null if that side hasn't happened yet (e.g., a milestone that was
/// resumed but whose original pause predates our indexing).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MilestonePauseHistory {
    /// Whether the milestone is currently paused (mirrors the top-level `paused`)
    pub currently_paused: bool,
    /// Most recent pause transaction
    pub last_pause_tx_hash: Option<String>,
    /// On-chain time of the last pause (`{unix, iso}`)
    pub last_pause_time: Option<ChainTime>,
    /// Most recent resume transaction
    pub last_resume_tx_hash: Option<String>,
    /// On-chain time of the last resume (`{unix, iso}`)
    pub last_resume_time: Option<ChainTime>,
}

/// Milestone completion details
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MilestoneCompletion {
    /// Completion transaction hash
    pub tx_hash: String,
    /// On-chain completion time (`{unix, iso}`)
    pub time: Option<ChainTime>,
    /// Completion description
    pub description: Option<String>,
    /// Evidence array
    pub evidence: Option<serde_json::Value>,
}

/// Milestone withdrawal details
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MilestoneWithdrawal {
    /// Withdrawal transaction hash
    pub tx_hash: String,
    /// On-chain withdrawal time (`{unix, iso}`)
    pub time: Option<ChainTime>,
    /// Withdrawn amount in lovelace
    pub amount_lovelace: Option<i64>,
}

/// Milestone archive info (present when milestone has been superseded by a modify event)
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MilestoneArchiveInfo {
    /// Transaction hash of the modify event that archived this milestone
    pub archived_by_tx_hash: Option<String>,
    /// On-chain archive time (`{unix, iso}`)
    pub archived_at: Option<ChainTime>,
    /// ID of the new milestone that replaced this one
    pub superseded_by_id: Option<i32>,
}

/// Project reference
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProjectReference {
    /// Project ID
    pub project_id: String,
    /// Project name
    pub project_name: Option<String>,
}

/// Database row for milestone
#[derive(Debug, FromRow)]
#[allow(dead_code)]
pub struct MilestoneRow {
    pub id: i32,
    pub project_db_id: i32,
    pub milestone_id: String,
    pub milestone_order: i32,
    pub label: Option<String>,
    pub description: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub amount_lovelace: Option<i64>,
    pub time_limit: Option<i64>,
    pub withdrawn: bool,
    pub evidence_provided: bool,
    pub paused: bool,
    pub archived: bool,
    pub complete_tx_hash: Option<String>,
    pub complete_time: Option<i64>,
    pub complete_description: Option<String>,
    pub evidence: Option<serde_json::Value>,
    pub withdraw_tx_hash: Option<String>,
    pub withdraw_time: Option<i64>,
    pub withdraw_amount: Option<i64>,
    pub archived_by_tx_hash: Option<String>,
    pub archived_at: Option<i64>,
    pub superseded_by: Option<i32>,
    pub last_pause_tx_hash: Option<String>,
    pub last_pause_time: Option<i64>,
    pub last_resume_tx_hash: Option<String>,
    pub last_resume_time: Option<i64>,
    pub project_id: String,
    pub project_name: Option<String>,
}

impl From<MilestoneRow> for MilestoneResponse {
    fn from(row: MilestoneRow) -> Self {
        let completion = row.complete_tx_hash.as_ref().map(|tx| MilestoneCompletion {
            tx_hash: tx.clone(),
            time: ChainTime::maybe_from_secs(row.complete_time),
            description: row.complete_description.clone(),
            evidence: row.evidence.clone(),
        });

        let withdrawal = row.withdraw_tx_hash.as_ref().map(|tx| MilestoneWithdrawal {
            tx_hash: tx.clone(),
            time: ChainTime::maybe_from_secs(row.withdraw_time),
            amount_lovelace: row.withdraw_amount,
        });

        let archive_info = if row.archived {
            Some(MilestoneArchiveInfo {
                archived_by_tx_hash: row.archived_by_tx_hash,
                archived_at: ChainTime::maybe_from_secs(row.archived_at),
                superseded_by_id: row.superseded_by,
            })
        } else {
            None
        };

        let pause_history = if row.last_pause_tx_hash.is_some() || row.last_resume_tx_hash.is_some() {
            Some(MilestonePauseHistory {
                currently_paused: row.paused,
                last_pause_tx_hash: row.last_pause_tx_hash.clone(),
                last_pause_time: ChainTime::maybe_from_secs(row.last_pause_time),
                last_resume_tx_hash: row.last_resume_tx_hash.clone(),
                last_resume_time: ChainTime::maybe_from_secs(row.last_resume_time),
            })
        } else {
            None
        };

        Self {
            id: row.id,
            milestone_id: row.milestone_id,
            milestone_order: row.milestone_order,
            label: row.label,
            description: row.description,
            acceptance_criteria: row.acceptance_criteria,
            amount_lovelace: row.amount_lovelace,
            time_limit: row.time_limit,
            withdrawn: row.withdrawn,
            evidence_provided: row.evidence_provided,
            paused: row.paused,
            archived: row.archived,
            completion,
            withdrawal,
            archive_info,
            pause_history,
            project: ProjectReference {
                project_id: row.project_id,
                project_name: row.project_name,
            },
        }
    }
}

// ============================================================================
// EVENTS
// ============================================================================

/// Event response with full context
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventResponse {
    /// Internal database ID
    pub id: i32,
    /// Transaction hash
    pub tx_hash: String,
    /// Slot number
    pub slot: Option<i64>,
    /// Block number
    pub block_number: Option<i64>,
    /// On-chain block time (`{unix, iso}`)
    pub block_time: Option<ChainTime>,
    /// Event type (publish/initialize/fund/complete/disburse/withdraw/pause/resume/modify/cancel/sweep/reorganize)
    pub event_type: String,
    /// Amount in lovelace. Set on `fund` and `withdraw` events; null otherwise.
    pub amount_lovelace: Option<i64>,
    /// Justification text. Set on `pause`, `cancel`, `modify` events; null otherwise.
    pub reason: Option<String>,
    /// TOM `{label, details}` object preserved as-is. Set on `disburse` events only.
    pub destination: Option<serde_json::Value>,
    /// Treasury context. Populated for treasury-level events
    /// (`publish`, `initialize`, `disburse`, `sweep`, `reorganize`); null on vendor-level events.
    pub treasury: Option<EventTreasuryContext>,
    /// Project context. Populated for vendor-level events; null on treasury-level events.
    pub project: Option<EventProjectContext>,
    /// Milestone context. Populated when the event resolves to a specific milestone
    /// (typically `complete`, `withdraw`); null otherwise.
    pub milestone: Option<EventMilestoneContext>,
    /// Raw metadata
    pub metadata_raw: Option<serde_json::Value>,
    /// Event created at
    pub created_at: Option<DateTime<Utc>>,
}

/// Treasury context for event
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventTreasuryContext {
    /// Contract instance
    pub contract_instance: String,
}

/// Project context for event
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventProjectContext {
    /// Project ID
    pub project_id: String,
    /// Project name
    pub project_name: Option<String>,
    /// Contract address
    pub contract_address: Option<String>,
}

/// Milestone context for event
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventMilestoneContext {
    /// Milestone ID
    pub milestone_id: String,
    /// Milestone label
    pub label: Option<String>,
    /// Milestone order
    pub milestone_order: Option<i32>,
}

/// Database row for event with context
#[derive(Debug, FromRow)]
pub struct EventWithContextRow {
    pub id: i32,
    pub tx_hash: String,
    pub slot: Option<i64>,
    pub block_number: Option<i64>,
    pub block_time: Option<i64>,
    pub event_type: String,
    pub amount_lovelace: Option<i64>,
    pub reason: Option<String>,
    pub destination: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: Option<DateTime<Utc>>,
    pub treasury_instance: Option<String>,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub project_address: Option<String>,
    pub milestone_id: Option<String>,
    pub milestone_label: Option<String>,
    pub milestone_order: Option<i32>,
}

impl From<EventWithContextRow> for EventResponse {
    fn from(row: EventWithContextRow) -> Self {
        let treasury = row.treasury_instance.as_ref().map(|inst| EventTreasuryContext {
            contract_instance: inst.clone(),
        });

        let project = row.project_id.as_ref().map(|pid| EventProjectContext {
            project_id: pid.clone(),
            project_name: row.project_name.clone(),
            contract_address: row.project_address.clone(),
        });

        let milestone = row.milestone_id.as_ref().map(|mid| EventMilestoneContext {
            milestone_id: mid.clone(),
            label: row.milestone_label.clone(),
            milestone_order: row.milestone_order,
        });

        Self {
            id: row.id,
            tx_hash: row.tx_hash,
            slot: row.slot,
            block_number: row.block_number,
            block_time: ChainTime::maybe_from_secs(row.block_time),
            event_type: row.event_type,
            amount_lovelace: row.amount_lovelace,
            reason: row.reason,
            destination: row.destination,
            treasury,
            project,
            milestone,
            metadata_raw: row.metadata,
            created_at: row.created_at,
        }
    }
}

// ============================================================================
// UTXOS
// ============================================================================

/// UTXO response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UtxoResponse {
    /// Transaction hash
    pub tx_hash: String,
    /// Output index
    pub output_index: i16,
    /// Address
    pub address: Option<String>,
    /// Address type (treasury/vendor_contract/vendor)
    pub address_type: Option<String>,
    /// Amount in lovelace
    pub lovelace_amount: Option<i64>,
    /// Creation slot
    pub slot: Option<i64>,
    /// Block number
    pub block_number: Option<i64>,
}

/// Database row for UTXO
#[derive(Debug, FromRow)]
pub struct UtxoRow {
    pub tx_hash: String,
    pub output_index: i16,
    pub address: Option<String>,
    pub address_type: Option<String>,
    pub lovelace_amount: Option<i64>,
    pub slot: Option<i64>,
    pub block_number: Option<i64>,
}

impl From<UtxoRow> for UtxoResponse {
    fn from(row: UtxoRow) -> Self {
        Self {
            tx_hash: row.tx_hash,
            output_index: row.output_index,
            address: row.address,
            address_type: row.address_type,
            lovelace_amount: row.lovelace_amount,
            slot: row.slot,
            block_number: row.block_number,
        }
    }
}

/// UTXO at the shared vendor contract, labeled with the owning project.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProjectUtxoResponse {
    /// Transaction hash
    pub tx_hash: String,
    /// Output index
    pub output_index: i16,
    /// Address (always the singleton PSSC for this endpoint)
    pub address: Option<String>,
    /// Amount in lovelace
    pub lovelace_amount: Option<i64>,
    /// Creation slot
    pub slot: Option<i64>,
    /// Block number
    pub block_number: Option<i64>,
    /// Internal project DB id
    pub project_db_id: i32,
    /// Logical project identifier (e.g., "EC-0008-25")
    pub project_id: String,
    /// Project display name
    pub project_name: Option<String>,
    /// Project status (active/paused/completed/cancelled)
    pub project_status: Option<String>,
}

/// Database row for a project-labeled vendor-contract UTXO.
#[derive(Debug, FromRow)]
pub struct ProjectUtxoRow {
    pub tx_hash: String,
    pub output_index: i16,
    pub address: Option<String>,
    pub lovelace_amount: Option<i64>,
    pub slot: Option<i64>,
    pub block_number: Option<i64>,
    pub project_db_id: i32,
    pub project_id: String,
    pub project_name: Option<String>,
    pub project_status: Option<String>,
}

impl From<ProjectUtxoRow> for ProjectUtxoResponse {
    fn from(row: ProjectUtxoRow) -> Self {
        Self {
            tx_hash: row.tx_hash,
            output_index: row.output_index,
            address: row.address,
            lovelace_amount: row.lovelace_amount,
            slot: row.slot,
            block_number: row.block_number,
            project_db_id: row.project_db_id,
            project_id: row.project_id,
            project_name: row.project_name,
            project_status: row.project_status,
        }
    }
}

// ============================================================================
// STATISTICS
// ============================================================================

/// Comprehensive statistics response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StatisticsResponse {
    /// Treasury (singleton TRSC) statistics
    pub treasury: TreasuryStats,
    /// Vendor contract (singleton shared PSSC) statistics
    pub vendor_contracts: VendorContractStats,
    /// Project statistics
    pub projects: ProjectStats,
    /// Milestone statistics
    pub milestones: MilestoneStats,
    /// Event statistics
    pub events: EventStats,
    /// Financial statistics
    pub financials: FinancialStats,
    /// Sync status
    pub sync: SyncStats,
}

/// Treasury statistics
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TreasuryStats {
    /// Total treasury contracts
    pub total_count: i64,
    /// Active treasury contracts
    pub active_count: i64,
    /// Number of disbursements made
    pub disbursed_count: i64,
}

/// Vendor contract (singleton) statistics — the shared PSSC every project sits at.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VendorContractStats {
    /// Total vendor contracts known to the API. Expected to be 1 for our deployment.
    pub total_count: i64,
    /// Shared PSSC script address (`addr1x...`). Null until the first fund event lands.
    pub address: Option<String>,
    /// Number of distinct projects bound to this vendor contract.
    pub project_count: i64,
    /// Total UTXOs ever observed at this address (regardless of spent state).
    pub utxo_history_count: i64,
    /// Currently unspent UTXOs at this address.
    pub unspent_utxo_count: i64,
    /// Sum of unspent lovelace held at this address.
    pub current_balance_lovelace: i64,
}

/// Project statistics
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ProjectStats {
    /// Total projects
    pub total_count: i64,
    /// Active projects
    pub active_count: i64,
    /// Completed projects
    pub completed_count: i64,
    /// Paused projects
    pub paused_count: i64,
    /// Cancelled projects
    pub cancelled_count: i64,
}

/// Milestone statistics
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MilestoneStats {
    /// Total milestones
    pub total_count: i64,
    /// Pending milestones
    pub pending_count: i64,
    /// Completed milestones
    pub completed_count: i64,
    /// Withdrawn milestones
    pub withdrawn_count: i64,
}

/// Event statistics
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventStats {
    /// Total on-chain TOM events (from yaci_store)
    pub on_chain_count: i64,
    /// Total processed events (in treasury schema)
    pub processed_count: i64,
    /// On-chain events by type
    pub by_type: std::collections::HashMap<String, i64>,
}

/// Financial statistics
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct FinancialStats {
    /// Total allocated to projects in lovelace
    pub total_allocated_lovelace: i64,
    /// Total withdrawn in lovelace
    pub total_withdrawn_lovelace: i64,
    /// Current total balance in lovelace (from UTXOs)
    pub current_balance_lovelace: i64,
}

/// Sync status statistics
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncStats {
    /// Last synced slot
    pub last_slot: Option<i64>,
    /// Last synced block
    pub last_block: Option<i64>,
    /// Last sync time
    pub last_updated: Option<DateTime<Utc>>,
}

// ============================================================================
// QUERY PARAMETERS
// ============================================================================

fn default_page() -> u32 { 1 }
fn default_limit() -> u32 { 50 }

/// Vendor contracts query parameters
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct ProjectsQuery {
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: u32,
    /// Items per page
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Filter by status (active/paused/completed/cancelled)
    pub status: Option<String>,
    /// Search in project_id, project_name, description
    pub search: Option<String>,
    /// Sort field (fund_time, project_id, project_name)
    pub sort: Option<String>,
    /// Sort order (asc/desc, default: desc)
    pub order: Option<String>,
    /// Filter by fund time (Unix timestamp, from)
    pub from_time: Option<i64>,
    /// Filter by fund time (Unix timestamp, to)
    pub to_time: Option<i64>,
}

/// Events query parameters
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct EventsQuery {
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: u32,
    /// Items per page
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Filter by event type
    #[serde(rename = "type")]
    pub event_type: Option<String>,
    /// Filter by project ID
    pub project_id: Option<String>,
    /// Filter by time (Unix timestamp, from)
    pub from_time: Option<i64>,
    /// Filter by time (Unix timestamp, to)
    pub to_time: Option<i64>,
    /// Full-text search across `reason`, `destination`, and raw `metadata` (case-insensitive substring match).
    pub q: Option<String>,
}

/// Recent events query parameters
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct RecentEventsQuery {
    /// Hours to look back (default: 24)
    #[serde(default = "default_hours")]
    pub hours: u32,
    /// Maximum number of events (default: 50)
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Filter by event type
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

fn default_hours() -> u32 { 24 }

/// Milestones query parameters
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct MilestonesQuery {
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: u32,
    /// Items per page
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Filter by withdrawn status
    pub withdrawn: Option<bool>,
    /// Filter by evidence_provided status
    pub evidence_provided: Option<bool>,
    /// Filter by archived status (defaults to false if not specified)
    pub archived: Option<bool>,
    /// Filter by project ID
    pub project_id: Option<String>,
    /// Sort field (milestone_order, complete_time, withdraw_time)
    pub sort: Option<String>,
    /// Filter by milestone time (Unix timestamp, from). Matches whichever of
    /// `complete_time` or `withdraw_time` is set on the milestone.
    pub from_time: Option<i64>,
    /// Filter by milestone time (Unix timestamp, to).
    pub to_time: Option<i64>,
}

/// Project events query parameters
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct ProjectEventsQuery {
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: u32,
    /// Items per page
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Filter by event type
    #[serde(rename = "type")]
    pub event_type: Option<String>,
}

/// Generic pagination query (used by sub-list endpoints that don't need other filters).
#[derive(Debug, Deserialize, ToSchema, IntoParams)]
pub struct PaginationQuery {
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: u32,
    /// Items per page (max 100)
    #[serde(default = "default_limit")]
    pub limit: u32,
}
