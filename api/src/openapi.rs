//! OpenAPI documentation configuration

use utoipa::OpenApi;

use crate::models::v1::{
    ApiResponse, ChainStatus, DatabaseStatus, EventMilestoneContext, EventProjectContext,
    EventResponse, EventStats, EventTreasuryContext, EventsQuery, FinancialStats,
    MilestoneArchiveInfo, MilestoneCompletion, MilestoneWithdrawal, MilestoneResponse,
    MilestoneStats, MilestonesSummary, MilestonesQuery, PaginatedResponse, Pagination,
    PaginationQuery, ProjectCurrentUtxo, ProjectDetail, ProjectEventsQuery, ProjectReference,
    ProjectStats, ProjectSummary, ProjectUtxoResponse, ProjectsQuery, RecentEventsQuery,
    ResponseMeta, StatisticsResponse, StatusResponse, SyncStats, SyncStatusBlock, TotalsBlock,
    TreasuryFinancials, TreasuryReference, TreasuryResponse, TreasuryStatistics, TreasuryStats,
    UtxoResponse, VendorContractProjectsBlock, VendorContractResponse, VendorContractStats,
    VendorFinancials,
};

use crate::routes::v1::{
    events, milestones, projects, statistics, status, treasury, vendor_contract,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Cardano Administration API",
        version = "2.0.0",
        description = "REST API for tracking Cardano treasury contracts and fund disbursements.\n\n## Overview\n\nThis API provides access to the treasury contract, the shared vendor contract, projects, milestones, and event history for the Cardano treasury system.\n\n## Key Concepts\n\n- **Treasury Contract (TRSC)**: The singleton on-chain reserve contract that holds funds.\n- **Vendor Contract (PSSC)**: The singleton on-chain script address every project sits at, distinguished only by inline datum.\n- **Project**: One row per `fund` event (e.g. `EC-0008-25`). 42 of these in our deployment.\n- **Milestone**: An individual deliverable within a project.\n- **Event**: Audit log of all treasury operations (fund, complete, disburse, etc.).\n\n## Response Format\n\nAll responses use a consistent envelope:\n\n```json\n{\n  \"data\": { ... },\n  \"pagination\": { ... },   // present on paginated endpoints\n  \"meta\":  { \"timestamp\": \"2026-05-01T10:30:00Z\" }\n}\n```\n\nErrors use a parallel envelope:\n\n```json\n{\n  \"error\": { \"code\": \"not_found\", \"message\": \"…\", \"details\": {…}? },\n  \"meta\":  { \"timestamp\": \"2026-05-01T10:30:00Z\" }\n}\n```\n\n## Amounts\n\nAll monetary amounts are in **lovelace** (the smallest unit; 1 ADA = 1,000,000 lovelace). Clients are responsible for ADA formatting.\n\n## Timestamps\n\nOn-chain block times are returned as a paired object: `{\"unix\": 1777609469, \"iso\": \"2025-09-29T12:24:29Z\"}`. Server-side timestamps (`created_at`, `updated_at`) are ISO 8601 strings.",
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
        (name = "Treasury", description = "Treasury contract (singleton TRSC) endpoints"),
        (name = "Vendor Contract", description = "Shared vendor contract (singleton PSSC) endpoint"),
        (name = "Projects", description = "Project endpoints (one per fund event)"),
        (name = "Milestones", description = "Milestone endpoints"),
        (name = "Events", description = "Event log endpoints"),
        (name = "Statistics", description = "Aggregated statistics endpoints")
    ),
    paths(
        status::get_status,
        treasury::get_treasury,
        treasury::get_treasury_utxos,
        treasury::get_treasury_events,
        vendor_contract::get_vendor_contract,
        vendor_contract::get_vendor_contract_utxos,
        projects::list_projects,
        projects::get_project,
        projects::get_project_milestones,
        projects::get_project_events,
        projects::get_project_utxos,
        milestones::list_milestones,
        milestones::list_milestones_by_project,
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
            ApiResponse<VendorContractResponse>,
            ApiResponse<ProjectDetail>,
            ApiResponse<Vec<MilestoneResponse>>,
            ApiResponse<Vec<UtxoResponse>>,
            ApiResponse<Vec<EventResponse>>,
            ApiResponse<EventResponse>,
            ApiResponse<MilestoneResponse>,
            ApiResponse<StatisticsResponse>,
            ApiResponse<StatusResponse>,
            PaginatedResponse<Vec<ProjectSummary>>,
            PaginatedResponse<Vec<MilestoneResponse>>,
            PaginatedResponse<Vec<EventResponse>>,
            PaginatedResponse<Vec<UtxoResponse>>,
            PaginatedResponse<Vec<ProjectUtxoResponse>>,
            Pagination,
            ResponseMeta,
            // Treasury
            TreasuryResponse,
            TreasuryStatistics,
            TreasuryFinancials,
            // Vendor Contract (singleton)
            VendorContractResponse,
            VendorContractProjectsBlock,
            // Projects
            ProjectSummary,
            ProjectDetail,
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
            ProjectUtxoResponse,
            ProjectCurrentUtxo,
            // Statistics
            StatisticsResponse,
            TreasuryStats,
            VendorContractStats,
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
            ProjectsQuery,
            EventsQuery,
            RecentEventsQuery,
            MilestonesQuery,
            ProjectEventsQuery,
            PaginationQuery,
        )
    )
)]
pub struct ApiDoc;
