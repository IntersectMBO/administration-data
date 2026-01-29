//! OpenAPI documentation configuration

use utoipa::OpenApi;

use crate::models::v1::{
    ApiResponse, EventMilestoneContext, EventProjectContext, EventResponse, EventStats,
    EventTreasuryContext, EventsQuery, FinancialStats, MilestoneCompletion, MilestoneDisbursement,
    MilestoneResponse, MilestoneStats, MilestonesSummary, MilestonesQuery, PaginatedResponse,
    Pagination, ProjectEventsQuery, ProjectReference, ProjectStats, RecentEventsQuery,
    ResponseMeta, StatisticsResponse, StatusResponse, SyncStats, TreasuryFinancials,
    TreasuryReference, TreasuryResponse, TreasuryStatistics, TreasuryStats, UtxoResponse,
    VendorContractDetail, VendorContractSummary, VendorContractsQuery, VendorFinancials,
};

use crate::routes::v1::{
    events, milestones, statistics, status, treasury, vendor_contracts,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Cardano Administration API",
        version = "1.0.0",
        description = "REST API for tracking Cardano treasury contracts and fund disbursements.\n\n## Overview\n\nThis API provides access to treasury contract data, vendor contracts (projects), milestones, and event history for the Cardano treasury system.\n\n## Key Concepts\n\n- **Treasury Contract (TRSC)**: The root treasury reserve contract that holds funds\n- **Vendor Contract (PSSC)**: Project-specific contracts that receive funding from the treasury\n- **Milestone**: Individual deliverables within a vendor contract\n- **Event**: Audit log of all treasury operations (fund, complete, disburse, etc.)\n\n## Response Format\n\nAll responses use a consistent envelope format:\n\n```json\n{\n  \"data\": { ... },\n  \"pagination\": { ... },  // Only for paginated endpoints\n  \"meta\": {\n    \"timestamp\": \"2026-01-28T10:30:00Z\"\n  }\n}\n```\n\n## Amounts\n\nAll monetary amounts are provided in both lovelace (smallest unit) and ADA:\n- `amount_lovelace`: Integer amount in lovelace\n- `amount_ada`: Float amount in ADA (1 ADA = 1,000,000 lovelace)",
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
            MilestoneDisbursement,
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
            // Query params
            VendorContractsQuery,
            EventsQuery,
            RecentEventsQuery,
            MilestonesQuery,
            ProjectEventsQuery,
        )
    )
)]
pub struct ApiDoc;
