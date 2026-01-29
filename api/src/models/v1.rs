//! V1 API Models with OpenAPI support
//!
//! These models follow the new API design with:
//! - Both lovelace AND ADA amounts in responses
//! - Raw metadata AND parsed/normalized data
//! - Consistent response envelopes with pagination

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::{IntoParams, ToSchema};

// ============================================================================
// CONSTANTS
// ============================================================================

/// Lovelace per ADA
pub const LOVELACE_PER_ADA: f64 = 1_000_000.0;

/// Convert lovelace to ADA
pub fn lovelace_to_ada(lovelace: i64) -> f64 {
    lovelace as f64 / LOVELACE_PER_ADA
}

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

/// API status response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StatusResponse {
    /// API version
    pub api_version: String,
    /// Database connection status
    pub database_connected: bool,
    /// Last sync slot
    pub last_sync_slot: Option<i64>,
    /// Last sync block
    pub last_sync_block: Option<i64>,
    /// Last sync time (Unix timestamp)
    pub last_sync_time: Option<i64>,
    /// Total events processed
    pub total_events: i64,
    /// Total vendor contracts
    pub total_vendor_contracts: i64,
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
    /// Human-readable name
    pub name: Option<String>,
    /// Contract status (active/paused)
    pub status: Option<String>,
    /// Publish transaction hash
    pub publish_tx_hash: Option<String>,
    /// Publish time (Unix timestamp)
    pub publish_time: Option<i64>,
    /// Initialize transaction hash
    pub initialized_tx_hash: Option<String>,
    /// Initialize time (Unix timestamp)
    pub initialized_at: Option<i64>,
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
    /// Total vendor contracts
    pub vendor_contract_count: i64,
    /// Active vendor contracts
    pub active_contracts: i64,
    /// Completed vendor contracts
    pub completed_contracts: i64,
    /// Cancelled vendor contracts
    pub cancelled_contracts: i64,
    /// Total events
    pub total_events: i64,
    /// Current UTXO count
    pub utxo_count: i64,
    /// Last event time (Unix timestamp)
    pub last_event_time: Option<i64>,
}

/// Treasury financial summary
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TreasuryFinancials {
    /// Treasury balance in lovelace
    pub balance_lovelace: i64,
    /// Treasury balance in ADA
    pub balance_ada: f64,
}

/// Database row for treasury summary
#[derive(Debug, FromRow)]
pub struct TreasurySummaryRow {
    pub treasury_id: i32,
    pub contract_instance: String,
    pub contract_address: Option<String>,
    pub stake_credential: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    pub publish_tx_hash: Option<String>,
    pub publish_time: Option<i64>,
    pub initialized_tx_hash: Option<String>,
    pub initialized_at: Option<i64>,
    pub permissions: Option<serde_json::Value>,
    pub vendor_contract_count: Option<i64>,
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
            name: row.name,
            status: row.status,
            publish_tx_hash: row.publish_tx_hash,
            publish_time: row.publish_time,
            initialized_tx_hash: row.initialized_tx_hash,
            initialized_at: row.initialized_at,
            permissions: row.permissions,
            statistics: TreasuryStatistics {
                vendor_contract_count: row.vendor_contract_count.unwrap_or(0),
                active_contracts: row.active_contracts.unwrap_or(0),
                completed_contracts: row.completed_contracts.unwrap_or(0),
                cancelled_contracts: row.cancelled_contracts.unwrap_or(0),
                total_events: row.total_events.unwrap_or(0),
                utxo_count: row.utxo_count.unwrap_or(0),
                last_event_time: row.last_event_time,
            },
            financials: TreasuryFinancials {
                balance_lovelace: balance,
                balance_ada: lovelace_to_ada(balance),
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
pub struct VendorContractSummary {
    /// Internal database ID
    pub id: i32,
    /// Logical project identifier (e.g., "EC-0008-25")
    pub project_id: String,
    /// Project name/label
    pub project_name: Option<String>,
    /// Project description
    pub description: Option<String>,
    /// Vendor name
    pub vendor_name: Option<String>,
    /// Vendor payment address
    pub vendor_address: Option<String>,
    /// Contract URL (link to agreement)
    pub contract_url: Option<String>,
    /// PSSC script address
    pub contract_address: Option<String>,
    /// Contract status (active/paused/completed/cancelled)
    pub status: Option<String>,
    /// Fund transaction hash
    pub fund_tx_hash: String,
    /// Fund time (Unix timestamp)
    pub fund_time: Option<i64>,
    /// Initial allocated amount in lovelace
    pub initial_amount_lovelace: Option<i64>,
    /// Initial allocated amount in ADA
    pub initial_amount_ada: Option<f64>,
    /// Milestone summary
    pub milestones_summary: MilestonesSummary,
    /// Financial summary
    pub financials: VendorFinancials,
    /// Treasury reference
    pub treasury: TreasuryReference,
    /// Last event time (Unix timestamp)
    pub last_event_time: Option<i64>,
    /// Total event count
    pub event_count: Option<i64>,
}

/// Vendor contract detail (full response)
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VendorContractDetail {
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
    /// Vendor name
    pub vendor_name: Option<String>,
    /// Vendor payment address
    pub vendor_address: Option<String>,
    /// Contract URL (link to agreement)
    pub contract_url: Option<String>,
    /// PSSC script address
    pub contract_address: Option<String>,
    /// Contract status (active/paused/completed/cancelled)
    pub status: Option<String>,
    /// Fund transaction hash
    pub fund_tx_hash: String,
    /// Fund time (Unix timestamp)
    pub fund_time: Option<i64>,
    /// Initial allocated amount in lovelace
    pub initial_amount_lovelace: Option<i64>,
    /// Initial allocated amount in ADA
    pub initial_amount_ada: Option<f64>,
    /// Milestone summary
    pub milestones_summary: MilestonesSummary,
    /// Financial summary
    pub financials: VendorFinancials,
    /// Treasury reference
    pub treasury: TreasuryReference,
    /// Last event time (Unix timestamp)
    pub last_event_time: Option<i64>,
    /// Total event count
    pub event_count: Option<i64>,
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
    /// Completed milestones (but not yet disbursed)
    pub completed: i64,
    /// Disbursed milestones
    pub disbursed: i64,
}

/// Vendor contract financial summary
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VendorFinancials {
    /// Total allocated amount in lovelace
    pub total_allocated_lovelace: i64,
    /// Total allocated amount in ADA
    pub total_allocated_ada: f64,
    /// Total disbursed amount in lovelace
    pub total_disbursed_lovelace: i64,
    /// Total disbursed amount in ADA
    pub total_disbursed_ada: f64,
    /// Current balance in lovelace (from UTXOs)
    pub current_balance_lovelace: i64,
    /// Current balance in ADA
    pub current_balance_ada: f64,
    /// Disbursement percentage
    pub disbursement_percentage: f64,
    /// UTXO count
    pub utxo_count: i64,
}

/// Treasury reference
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TreasuryReference {
    /// Contract instance identifier
    pub contract_instance: Option<String>,
    /// Treasury name
    pub name: Option<String>,
}

/// Database row for vendor contract summary
#[derive(Debug, FromRow)]
#[allow(dead_code)]
pub struct VendorContractSummaryRow {
    pub id: i32,
    pub treasury_id: Option<i32>,
    pub project_id: String,
    pub other_identifiers: Option<Vec<String>>,
    pub project_name: Option<String>,
    pub description: Option<String>,
    pub vendor_name: Option<String>,
    pub vendor_address: Option<String>,
    pub contract_url: Option<String>,
    pub contract_address: Option<String>,
    pub fund_tx_hash: String,
    pub fund_slot: Option<i64>,
    pub fund_block_time: Option<i64>,
    pub initial_amount_lovelace: Option<i64>,
    pub status: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub treasury_instance: Option<String>,
    pub treasury_name: Option<String>,
    pub total_milestones: Option<i64>,
    pub pending_milestones: Option<i64>,
    pub completed_milestones: Option<i64>,
    pub disbursed_milestones: Option<i64>,
    pub total_disbursed_lovelace: Option<i64>,
    pub current_balance_lovelace: Option<i64>,
    pub utxo_count: Option<i64>,
    pub last_event_time: Option<i64>,
    pub event_count: Option<i64>,
}

impl From<VendorContractSummaryRow> for VendorContractSummary {
    fn from(row: VendorContractSummaryRow) -> Self {
        let initial_amount = row.initial_amount_lovelace.unwrap_or(0);
        let total_disbursed = row.total_disbursed_lovelace.unwrap_or(0);
        let current_balance = row.current_balance_lovelace.unwrap_or(0);
        let disbursement_pct = if initial_amount > 0 {
            (total_disbursed as f64 / initial_amount as f64) * 100.0
        } else {
            0.0
        };

        Self {
            id: row.id,
            project_id: row.project_id,
            project_name: row.project_name,
            description: row.description,
            vendor_name: row.vendor_name,
            vendor_address: row.vendor_address,
            contract_url: row.contract_url,
            contract_address: row.contract_address,
            status: row.status,
            fund_tx_hash: row.fund_tx_hash,
            fund_time: row.fund_block_time,
            initial_amount_lovelace: row.initial_amount_lovelace,
            initial_amount_ada: row.initial_amount_lovelace.map(lovelace_to_ada),
            milestones_summary: MilestonesSummary {
                total: row.total_milestones.unwrap_or(0),
                pending: row.pending_milestones.unwrap_or(0),
                completed: row.completed_milestones.unwrap_or(0),
                disbursed: row.disbursed_milestones.unwrap_or(0),
            },
            financials: VendorFinancials {
                total_allocated_lovelace: initial_amount,
                total_allocated_ada: lovelace_to_ada(initial_amount),
                total_disbursed_lovelace: total_disbursed,
                total_disbursed_ada: lovelace_to_ada(total_disbursed),
                current_balance_lovelace: current_balance,
                current_balance_ada: lovelace_to_ada(current_balance),
                disbursement_percentage: disbursement_pct,
                utxo_count: row.utxo_count.unwrap_or(0),
            },
            treasury: TreasuryReference {
                contract_instance: row.treasury_instance,
                name: row.treasury_name,
            },
            last_event_time: row.last_event_time,
            event_count: row.event_count,
        }
    }
}

impl From<VendorContractSummaryRow> for VendorContractDetail {
    fn from(row: VendorContractSummaryRow) -> Self {
        let initial_amount = row.initial_amount_lovelace.unwrap_or(0);
        let total_disbursed = row.total_disbursed_lovelace.unwrap_or(0);
        let current_balance = row.current_balance_lovelace.unwrap_or(0);
        let disbursement_pct = if initial_amount > 0 {
            (total_disbursed as f64 / initial_amount as f64) * 100.0
        } else {
            0.0
        };

        Self {
            id: row.id,
            project_id: row.project_id,
            other_identifiers: row.other_identifiers,
            project_name: row.project_name,
            description: row.description,
            vendor_name: row.vendor_name,
            vendor_address: row.vendor_address,
            contract_url: row.contract_url,
            contract_address: row.contract_address,
            status: row.status,
            fund_tx_hash: row.fund_tx_hash,
            fund_time: row.fund_block_time,
            initial_amount_lovelace: row.initial_amount_lovelace,
            initial_amount_ada: row.initial_amount_lovelace.map(lovelace_to_ada),
            milestones_summary: MilestonesSummary {
                total: row.total_milestones.unwrap_or(0),
                pending: row.pending_milestones.unwrap_or(0),
                completed: row.completed_milestones.unwrap_or(0),
                disbursed: row.disbursed_milestones.unwrap_or(0),
            },
            financials: VendorFinancials {
                total_allocated_lovelace: initial_amount,
                total_allocated_ada: lovelace_to_ada(initial_amount),
                total_disbursed_lovelace: total_disbursed,
                total_disbursed_ada: lovelace_to_ada(total_disbursed),
                current_balance_lovelace: current_balance,
                current_balance_ada: lovelace_to_ada(current_balance),
                disbursement_percentage: disbursement_pct,
                utxo_count: row.utxo_count.unwrap_or(0),
            },
            treasury: TreasuryReference {
                contract_instance: row.treasury_instance,
                name: row.treasury_name,
            },
            last_event_time: row.last_event_time,
            event_count: row.event_count,
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
    /// Allocated amount in ADA
    pub amount_ada: Option<f64>,
    /// Milestone status (pending/completed/disbursed)
    pub status: String,
    /// Completion details
    pub completion: Option<MilestoneCompletion>,
    /// Disbursement details
    pub disbursement: Option<MilestoneDisbursement>,
    /// Project reference
    pub project: ProjectReference,
}

/// Milestone completion details
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MilestoneCompletion {
    /// Completion transaction hash
    pub tx_hash: String,
    /// Completion time (Unix timestamp)
    pub time: Option<i64>,
    /// Completion description
    pub description: Option<String>,
    /// Evidence array
    pub evidence: Option<serde_json::Value>,
}

/// Milestone disbursement details
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct MilestoneDisbursement {
    /// Disbursement transaction hash
    pub tx_hash: String,
    /// Disbursement time (Unix timestamp)
    pub time: Option<i64>,
    /// Disbursed amount in lovelace
    pub amount_lovelace: Option<i64>,
    /// Disbursed amount in ADA
    pub amount_ada: Option<f64>,
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
    pub vendor_contract_id: i32,
    pub milestone_id: String,
    pub milestone_order: i32,
    pub label: Option<String>,
    pub description: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub amount_lovelace: Option<i64>,
    pub status: String,
    pub complete_tx_hash: Option<String>,
    pub complete_time: Option<i64>,
    pub complete_description: Option<String>,
    pub evidence: Option<serde_json::Value>,
    pub disburse_tx_hash: Option<String>,
    pub disburse_time: Option<i64>,
    pub disburse_amount: Option<i64>,
    pub project_id: String,
    pub project_name: Option<String>,
}

impl From<MilestoneRow> for MilestoneResponse {
    fn from(row: MilestoneRow) -> Self {
        let completion = row.complete_tx_hash.as_ref().map(|tx| MilestoneCompletion {
            tx_hash: tx.clone(),
            time: row.complete_time,
            description: row.complete_description.clone(),
            evidence: row.evidence.clone(),
        });

        let disbursement = row.disburse_tx_hash.as_ref().map(|tx| MilestoneDisbursement {
            tx_hash: tx.clone(),
            time: row.disburse_time,
            amount_lovelace: row.disburse_amount,
            amount_ada: row.disburse_amount.map(lovelace_to_ada),
        });

        Self {
            id: row.id,
            milestone_id: row.milestone_id,
            milestone_order: row.milestone_order,
            label: row.label,
            description: row.description,
            acceptance_criteria: row.acceptance_criteria,
            amount_lovelace: row.amount_lovelace,
            amount_ada: row.amount_lovelace.map(lovelace_to_ada),
            status: row.status,
            completion,
            disbursement,
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
    /// Block time (Unix timestamp)
    pub block_time: Option<i64>,
    /// Event type (publish/initialize/fund/complete/disburse/etc.)
    pub event_type: String,
    /// Amount in lovelace (if applicable)
    pub amount_lovelace: Option<i64>,
    /// Amount in ADA (if applicable)
    pub amount_ada: Option<f64>,
    /// Reason (for pause/cancel/modify events)
    pub reason: Option<String>,
    /// Destination (for disburse events)
    pub destination: Option<String>,
    /// Treasury context
    pub treasury: Option<EventTreasuryContext>,
    /// Project context
    pub project: Option<EventProjectContext>,
    /// Milestone context
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
    /// Treasury name
    pub name: Option<String>,
}

/// Project context for event
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct EventProjectContext {
    /// Project ID
    pub project_id: String,
    /// Project name
    pub project_name: Option<String>,
    /// Vendor name
    pub vendor_name: Option<String>,
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
    pub destination: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: Option<DateTime<Utc>>,
    pub treasury_instance: Option<String>,
    pub treasury_name: Option<String>,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub vendor_name: Option<String>,
    pub project_address: Option<String>,
    pub milestone_id: Option<String>,
    pub milestone_label: Option<String>,
    pub milestone_order: Option<i32>,
}

impl From<EventWithContextRow> for EventResponse {
    fn from(row: EventWithContextRow) -> Self {
        let treasury = row.treasury_instance.as_ref().map(|inst| EventTreasuryContext {
            contract_instance: inst.clone(),
            name: row.treasury_name.clone(),
        });

        let project = row.project_id.as_ref().map(|pid| EventProjectContext {
            project_id: pid.clone(),
            project_name: row.project_name.clone(),
            vendor_name: row.vendor_name.clone(),
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
            block_time: row.block_time,
            event_type: row.event_type,
            amount_lovelace: row.amount_lovelace,
            amount_ada: row.amount_lovelace.map(lovelace_to_ada),
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
    /// Amount in ADA
    pub ada_amount: Option<f64>,
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
            ada_amount: row.lovelace_amount.map(lovelace_to_ada),
            slot: row.slot,
            block_number: row.block_number,
        }
    }
}

// ============================================================================
// STATISTICS
// ============================================================================

/// Comprehensive statistics response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct StatisticsResponse {
    /// Treasury statistics
    pub treasury: TreasuryStats,
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
    /// Total events
    pub total_count: i64,
    /// Events by type
    pub by_type: std::collections::HashMap<String, i64>,
}

/// Financial statistics
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct FinancialStats {
    /// Total allocated to projects in lovelace
    pub total_allocated_lovelace: i64,
    /// Total allocated to projects in ADA
    pub total_allocated_ada: f64,
    /// Total disbursed in lovelace
    pub total_disbursed_lovelace: i64,
    /// Total disbursed in ADA
    pub total_disbursed_ada: f64,
    /// Current total balance in lovelace (from UTXOs)
    pub current_balance_lovelace: i64,
    /// Current total balance in ADA
    pub current_balance_ada: f64,
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
pub struct VendorContractsQuery {
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: u32,
    /// Items per page
    #[serde(default = "default_limit")]
    pub limit: u32,
    /// Filter by status (active/paused/completed/cancelled)
    pub status: Option<String>,
    /// Search in project_id, project_name, description, vendor_name
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
    /// Filter by status (pending/completed/disbursed)
    pub status: Option<String>,
    /// Filter by project ID
    pub project_id: Option<String>,
    /// Sort field (milestone_order, complete_time, disburse_time)
    pub sort: Option<String>,
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
