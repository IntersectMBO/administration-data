//! OpenAPI documentation configuration

use utoipa::OpenApi;

use crate::models::v1::{
    ApiResponse, ChainStatus, DatabaseStatus, EventMilestoneContext, EventProjectContext,
    EventResponse, EventStats, EventTreasuryContext, EventsQuery, FinancialStats,
    MilestoneArchiveInfo, MilestoneCompletion, MilestoneWithdrawal, MilestoneResponse,
    MilestoneStats, MilestonesSummary, MilestonesQuery, PaginatedResponse, Pagination,
    PaginationQuery, ProjectEventsQuery, ProjectReference, ProjectStats, RecentEventsQuery,
    ResponseMeta, StatisticsResponse, StatusResponse, SyncStats, SyncStatusBlock, TotalsBlock,
    TreasuryFinancials, TreasuryReference, TreasuryResponse, TreasuryStatistics, TreasuryStats,
    UtxoResponse, VendorContractDetail, VendorContractSummary, VendorContractsQuery,
    VendorFinancials,
};

use crate::routes::v1::{
    events, milestones, statistics, status, treasury, vendor_contracts,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Cardano Administration API",
        version = "1.1.0",
        description = "REST API for tracking Cardano treasury contracts and fund disbursements.\n\n## Overview\n\nThis API provides access to treasury contract data, vendor contracts (projects), milestones, and event history for the Cardano treasury system.\n\n## Key Concepts\n\n- **Treasury Contract (TRSC)**: The root treasury reserve contract that holds funds\n- **Vendor Contract (PSSC)**: Project-specific contracts that receive funding from the treasury\n- **Milestone**: Individual deliverables within a vendor contract\n- **Event**: Audit log of all treasury operations (fund, complete, disburse, etc.)\n\n## Response Format\n\nAll responses use a consistent envelope:\n\n```json\n{\n  \"data\": { ... },\n  \"pagination\": { ... },   // present on paginated endpoints\n  \"meta\":  { \"timestamp\": \"2026-05-01T10:30:00Z\" }\n}\n```\n\nErrors use a parallel envelope:\n\n```json\n{\n  \"error\": { \"code\": \"not_found\", \"message\": \"…\", \"details\": {…}? },\n  \"meta\":  { \"timestamp\": \"2026-05-01T10:30:00Z\" }\n}\n```\n\n## Amounts\n\nAll monetary amounts are in **lovelace** (the smallest unit; 1 ADA = 1,000,000 lovelace). Clients are responsible for ADA formatting.\n\n## Timestamps\n\nOn-chain block times are returned as a paired object: `{\"unix\": 1777609469, \"iso\": \"2025-09-29T12:24:29Z\"}`. Server-side timestamps (`created_at`, `updated_at`) are ISO 8601 strings.",
        license(
            name = "Apache 2.0",
            url = "https://www.apache.org/licenses/LICENSE-2.0"
        ),
        contact(
            name = "Cardano Treasury Team"
        )
    ),
    servers(
        (url = "/", description = "Local development server")
    ),
    tags(
        (name = "Status", description = "API health and status endpoints"),
        (name = "Treasury", description = "Treasury contract endpoints"),
        (name = "Vendor Contracts", description = "Vendor contract (project) endpoints"),
        (name = "Milestones", description = "Milestone endpoints"),
        (name = "Events", description = "Event log endpoints"),
        (name = "Statistics", description = "Aggregated statistics endpoints")
    ),
    paths(
        status::get_status,
        treasury::get_treasury,
        treasury::get_treasury_utxos,
        treasury::get_treasury_events,
        vendor_contracts::list_vendor_contracts,
        vendor_contracts::get_vendor_contract,
        vendor_contracts::get_vendor_contract_milestones,
        vendor_contracts::get_vendor_contract_events,
        vendor_contracts::get_vendor_contract_utxos,
        milestones::list_milestones,
        milestones::get_milestone,
        events::list_events,
        events::get_recent_events,
        events::get_event,
        statistics::get_statistics,
    ),
    components(
        schemas(
            // Response envelopes
            ApiResponse<TreasuryResponse>,
            ApiResponse<VendorContractDetail>,
            ApiResponse<Vec<MilestoneResponse>>,
            ApiResponse<Vec<UtxoResponse>>,
            ApiResponse<Vec<EventResponse>>,
            ApiResponse<EventResponse>,
            ApiResponse<MilestoneResponse>,
            ApiResponse<StatisticsResponse>,
            ApiResponse<StatusResponse>,
            PaginatedResponse<Vec<VendorContractSummary>>,
            PaginatedResponse<Vec<MilestoneResponse>>,
            PaginatedResponse<Vec<EventResponse>>,
            Pagination,
            ResponseMeta,
            // Treasury
            TreasuryResponse,
            TreasuryStatistics,
            TreasuryFinancials,
            // Vendor Contracts
            VendorContractSummary,
            VendorContractDetail,
            VendorFinancials,
            MilestonesSummary,
            TreasuryReference,
            // Milestones
            MilestoneResponse,
            MilestoneCompletion,
            MilestoneWithdrawal,
            MilestoneArchiveInfo,
            ProjectReference,
            // Events
            EventResponse,
            EventTreasuryContext,
            EventProjectContext,
            EventMilestoneContext,
            // UTXOs
            UtxoResponse,
            // Statistics
            StatisticsResponse,
            TreasuryStats,
            ProjectStats,
            MilestoneStats,
            EventStats,
            FinancialStats,
            SyncStats,
            // Status
            StatusResponse,
            DatabaseStatus,
            SyncStatusBlock,
            ChainStatus,
            TotalsBlock,
            // Errors
            crate::errors::ApiErrorBody,
            crate::errors::ApiErrorDetail,
            // Time
            crate::models::time::ChainTime,
            // Query params
            VendorContractsQuery,
            EventsQuery,
            RecentEventsQuery,
            MilestonesQuery,
            ProjectEventsQuery,
            PaginationQuery,
        )
    )
)]
pub struct ApiDoc;
