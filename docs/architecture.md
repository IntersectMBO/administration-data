# Architecture & Data Flow Documentation

This document describes how data flows through the Cardano Administration Data System.

## System Overview

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              CARDANO MAINNET                                    │
│                         (backbone.cardano.iog.io:3001)                          │
└─────────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       │ N2N Protocol
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           YACI STORE INDEXER                                    │
│                              (Port 8081)                                        │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │   Block     │───▶│   Plugin    │───▶│   Filter    │───▶│  Database   │      │
│  │  Fetcher    │    │   Engine    │    │   Scripts   │    │   Writer    │      │
│  └─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘      │
└─────────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       │ JDBC
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              POSTGRESQL                                         │
│                              (Port 5433)                                        │
│                                                                                 │
│   ┌─────────────────────────────┐    ┌─────────────────────────────┐           │
│   │      yaci_store schema      │    │      treasury schema        │           │
│   │  (raw blockchain data)      │    │  (normalized app data)      │           │
│   │                             │    │                             │           │
│   │  • block                    │    │  • treasury_contracts       │           │
│   │  • transaction              │    │  • vendor_contracts (PSSC)  │           │
│   │  • address_utxo             │    │  • projects                 │           │
│   │  • transaction_metadata     │    │  • milestones               │           │
│   │  • tx_input                 │    │  • events                   │           │
│   │                             │    │  • utxo_history             │           │
│   └─────────────────────────────┘    └─────────────────────────────┘           │
└─────────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       │ SQLx
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              RUST API                                           │
│                              (Port 8080)                                        │
│                                                                                 │
│   ┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐            │
│   │  Sync Service   │    │ Event Processor │    │  REST Endpoints │            │
│   │  (background)   │───▶│  (transforms)   │    │  (serves data)  │            │
│   └─────────────────┘    └─────────────────┘    └─────────────────┘            │
└─────────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       │ HTTP/JSON
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                                  CLIENTS                                        │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## Data Flow Stages

### Stage 1: Blockchain Indexing (YACI Store)

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         BLOCK PROCESSING PIPELINE                            │
└──────────────────────────────────────────────────────────────────────────────┘

  Cardano Node                    YACI Store Indexer
       │
       │  Block Data
       ▼
  ┌─────────┐
  │  Block  │──────────────────────────────────────────────────────────────┐
  │  Header │                                                              │
  └─────────┘                                                              │
       │                                                                   │
       ▼                                                                   ▼
  ┌─────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────────┐
  │  Txs    │────▶│   Extract   │────▶│   FILTER    │────▶│  yaci_store.    │
  │         │     │   UTXOs     │     │  (plugin)   │     │  address_utxo   │
  └─────────┘     └─────────────┘     └─────────────┘     └─────────────────┘
       │                                     │
       │                              Only treasury
       │                              addresses pass
       │
       ▼
  ┌─────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────────┐
  │Metadata │────▶│   Extract   │────▶│   FILTER    │────▶│  yaci_store.    │
  │         │     │  Label 1694 │     │  (plugin)   │     │  tx_metadata    │
  └─────────┘     └─────────────┘     └─────────────┘     └─────────────────┘
                                             │
                                      Only label 1694
                                      (TOM) passes
                                             │
                                             ▼
                                      ┌─────────────┐
                                      │ POST-ACTION │
                                      │   Log tx    │
                                      └─────────────┘
```

### Stage 2: Plugin Filter Logic

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                           UTXO FILTER (treasury-filter.mvel)                 │
└──────────────────────────────────────────────────────────────────────────────┘

                    ┌─────────────────────────────────────┐
                    │         Incoming UTXO               │
                    │  • ownerAddr                        │
                    │  • ownerStakeCredential             │
                    │  • lovelaceAmount                   │
                    └─────────────────────────────────────┘
                                      │
                                      ▼
                    ┌─────────────────────────────────────┐
                    │   Is ownerAddr in known addresses?  │
                    └─────────────────────────────────────┘
                           │                    │
                          YES                   NO
                           │                    │
                           │                    ▼
                           │    ┌─────────────────────────────────────┐
                           │    │  Does stakeCredential match         │
                           │    │  treasury script hash?              │
                           │    │  (8583857e4a12ffe1e6f641...)        │
                           │    └─────────────────────────────────────┘
                           │                    │
                           │           YES      │      NO
                           │            │       │       │
                           │            │       │       ▼
                           │            │       │   ┌───────┐
                           │            │       │   │ SKIP  │
                           │            │       │   └───────┘
                           │            │       │
                           ▼            ▼       │
                    ┌─────────────────────────────────────┐
                    │              KEEP UTXO              │
                    │  + Add address to known_addresses   │
                    └─────────────────────────────────────┘


┌──────────────────────────────────────────────────────────────────────────────┐
│                           METADATA FILTER                                    │
└──────────────────────────────────────────────────────────────────────────────┘

                    ┌─────────────────────────────────────┐
                    │       Incoming Metadata             │
                    │  • label                            │
                    │  • body (JSON with "instance")      │
                    └─────────────────────────────────────┘
                                      │
                                      ▼
                    ┌─────────────────────────────────────┐
                    │         label == "1694" ?           │
                    └─────────────────────────────────────┘
                           │                    │
                          YES                   NO
                           │                    │
                           ▼                    ▼
                    ┌─────────────────────────────────────┐
                    │  instance == TREASURY_INSTANCE ?    │
                    │  (from env variable)                │
                    └─────────────────────────────────────┘
                           │                    │
                          YES                   NO
                           │                    │
                           ▼                    ▼
                    ┌───────────┐         ┌───────────┐
                    │   KEEP    │         │   SKIP    │
                    └───────────┘         └───────────┘
```

### Stage 3: API Sync Service (Rust)

The background sync task (`api/src/services/sync.rs::run_sync_loop`) does
three things every 15 seconds:

1. Reads `treasury.sync_status` to find `last_slot` for `sync_type = 'events'`.
2. Selects new label-`1694` rows from `yaci_store.transaction_metadata` past
   that slot, plus a one-shot pre-fetch of their UTXOs into
   `treasury.utxo_history` via `EventProcessor::pre_fetch_utxos`. This is
   a defensive backstop on top of the Postgres triggers
   (`install_utxo_history_triggers` in `api/src/services/sync.rs`) that
   capture every script-address UTXO into `treasury.utxo_history`
   synchronously with YACI Store's INSERT, regardless of pruning. This is
   what makes pause/resume datum parsing and chain tracing keep working
   long after the on-chain UTXO is gone.
3. Dispatches each event through the per-type handler and advances
   `treasury.sync_status` only on contiguous success (any failed event
   wedges the watermark). A separate task runs `sync_all_events` every 10
   minutes as an idempotent backfill via `ON CONFLICT DO UPDATE`.

> **Caveat — `last_slot` advancement on errors.** If a single event fails
> mid-batch (e.g. DB connection reset), the loop logs and continues; later
> successful events advance `last_slot` past the failed one, so it is never
> retried by the continuous loop. Restarting the API runs `sync_all_events`
> from the beginning, which is idempotent (`ON CONFLICT (tx_hash) DO UPDATE`)
> and recovers the missed rows. Tracked as
> [`KI-SY-02`](known-issues.md#ki-sy-02--last_slot-can-advance-past-failed-events-on-connection-reset).

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                    BACKGROUND SYNC LOOP (every 15 seconds)                   │
└──────────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────────┐
│                           yaci_store schema                                  │
│                                                                              │
│   transaction_metadata                                                       │
│   ┌──────────────────────────────────────────────────────────────────────┐  │
│   │ tx_hash | slot | label | body (JSON)                                 │  │
│   │─────────────────────────────────────────────────────────────────────│  │
│   │ abc123  | 1000 | 1694  | {"body":{"event":"fund",...}}               │  │
│   │ def456  | 1050 | 1694  | {"body":{"event":"complete",...}}           │  │
│   └──────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ pre_fetch_utxos(batch tx_hashes)
                                      │ ──► treasury.utxo_history (raw output rows
                                      │       captured before YACI prunes; primary
                                      │       capture is via Postgres triggers)
                                      │
                                      │ SELECT WHERE slot > last_synced_slot
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          EVENT PROCESSOR                                     │
│                                                                              │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                     Parse event type from JSON                       │   │
│   │                     body.body.event = "fund" | "complete" | ...      │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
│                                      │                                       │
│        ┌─────────────┬───────────────┼───────────────┬─────────────┐        │
│        ▼             ▼               ▼               ▼             ▼        │
│   ┌─────────┐  ┌──────────┐  ┌────────────┐  ┌──────────┐  ┌──────────┐    │
│   │ publish │  │initialize│  │    fund    │  │ complete │  │ withdraw │    │
│   └─────────┘  └──────────┘  └────────────┘  └──────────┘  └──────────┘    │
│        │             │               │               │             │        │
│        ▼             ▼               ▼               ▼             ▼        │
│   ┌─────────────────────────────────────────────────────────────────────┐   │
│   │                    INSERT/UPDATE treasury schema                     │   │
│   └─────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           treasury schema                                    │
│                                                                              │
│   treasury_contracts   projects              milestones        events        │
│   ┌───────────────┐    ┌───────────────┐    ┌───────────────┐ ┌──────────┐  │
│   │ id            │    │ id            │    │ id            │ │ id       │  │
│   │ instance      │◄───│ treasury_id   │◄───│ project_db_id │ │ tx_hash  │  │
│   │ stake_cred    │    │ project_id    │    │ label         │ │ event    │  │
│   │ publish_tx    │    │ project_name  │    │ withdrawn     │ │ metadata │  │
│   └───────────────┘    │ status        │    │ evidence_*    │ └──────────┘  │
│                        └───────────────┘    │ paused        │                │
│   vendor_contracts                          │ archived      │                │
│   ┌───────────────┐                         │ superseded_by │                │
│   │ id            │                         └───────────────┘                │
│   │ address (PSSC)│                                                          │
│   │ stake_cred    │                                                          │
│   └───────────────┘                                                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Stage 4: Event Processing Detail

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         "fund" EVENT PROCESSING                              │
└──────────────────────────────────────────────────────────────────────────────┘

   TOM Metadata (label 1694)
   ┌────────────────────────────────────────────────────────────────────────┐
   │ {                                                                      │
   │   "instance": "9e65e4ed...",                                          │
   │   "body": {                                                           │
   │     "event": "fund",                                                  │
   │     "identifier": "project-001",                                      │
   │     "label": "My Project",                                            │
   │     "description": "Project description...",                          │
   │     "vendor": { "label": "addr1q..." },                                │
   │     "milestones": [                                                   │
   │       { "identifier": "m1", "label": "Phase 1", "amount": 1000000 },  │
   │       { "identifier": "m2", "label": "Phase 2", "amount": 2000000 }   │
   │     ]                                                                 │
   │   }                                                                   │
   │ }                                                                     │
   └────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ process_fund()
                                      ▼
   ┌────────────────────────────────────────────────────────────────────────┐
   │                                                                        │
   │  1. UPSERT treasury_contracts (by instance)                           │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.treasury_contracts (contract_instance)  │  │
   │     │ VALUES ('9e65e4ed...') ON CONFLICT DO UPDATE                 │  │
   │     └──────────────────────────────────────────────────────────────┘  │
   │                                      │                                 │
   │                                      ▼                                 │
   │  2. UPSERT vendor_contracts (singleton PSSC row at the shared addr)   │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.vendor_contracts (address, ...)         │  │
   │     │ ON CONFLICT (address) DO NOTHING                              │  │
   │     └──────────────────────────────────────────────────────────────┘  │
   │                                      │                                 │
   │                                      ▼                                 │
   │  3. INSERT projects                                                   │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.projects                                │  │
   │     │   (project_id, project_name, vendor_address, ...)             │  │
   │     │ VALUES ('project-001', 'My Project', 'addr1q...', ...)       │  │
   │     └──────────────────────────────────────────────────────────────┘  │
   │                                      │                                 │
   │                                      ▼                                 │
   │  4. INSERT milestones (for each milestone in array)                   │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.milestones                              │  │
   │     │   (project_db_id, milestone_id, label, amount)               │  │
   │     │ VALUES (1, 'm1', 'Phase 1', 1000000)                         │  │
   │     │ VALUES (1, 'm2', 'Phase 2', 2000000)                         │  │
   │     └──────────────────────────────────────────────────────────────┘  │
   │                                      │                                 │
   │                                      ▼                                 │
   │  5. INSERT event record                                               │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.events                                  │  │
   │     │   (tx_hash, event_type, project_db_id, metadata)             │  │
   │     └──────────────────────────────────────────────────────────────┘  │
   │                                      │                                 │
   │                                      ▼                                 │
   │  6. Track UTXOs for future event lookups                              │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.utxo_history (tx_hash, output_index,    │  │
   │     │                                project_db_id, spent)          │  │
   │     └──────────────────────────────────────────────────────────────┘  │
   │                                                                        │
   └────────────────────────────────────────────────────────────────────────┘
```

### Stage 5: UTXO Chain Tracking

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                    UTXO CHAIN TRACKING FOR EVENT LINKING                     │
└──────────────────────────────────────────────────────────────────────────────┘

   Problem: Events after "fund" don't always include project_id in metadata.
   Solution: Track which UTXOs belong to which project.

   TIME ──────────────────────────────────────────────────────────────────────▶

   ┌─────────────────────────────────────────────────────────────────────────┐
   │                         FUND TRANSACTION                                 │
   │  tx_hash: "abc123"                                                      │
   │  metadata: { "identifier": "project-001", "event": "fund" }             │
   │                                                                         │
   │  outputs:                                                               │
   │    [0] → UTXO₁ (contract address, 10,000 ADA)                          │
   │                                                                         │
   │  ──► Record in treasury.utxo_history:                                   │
   │      (tx_hash="abc123", output_index=0, project_db_id=1)               │
   └─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ UTXO₁ is spent
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                       COMPLETE TRANSACTION                              │
   │  tx_hash: "def456"                                                      │
   │  metadata: { "event": "complete", "milestone": "m1" }                   │
   │            (NO project_id!)                                             │
   │                                                                         │
   │  inputs:                                                                │
   │    [0] ← UTXO₁ (spending abc123:0)                                     │
   │                                                                         │
   │  outputs:                                                               │
   │    [0] → UTXO₂ (contract address, 9,000 ADA)                           │
   │                                                                         │
   │  ──► find_project_from_inputs("def456"):                                │
   │      1. Get inputs: [(abc123, 0)]                                      │
   │      2. Lookup treasury.utxo_history WHERE tx_hash="abc123" AND index=0│
   │      3. Found! project_db_id = 1                                       │
   │      4. Mark UTXO₁ as spent, record UTXO₂ with project_db_id=1         │
   └─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ UTXO₂ is spent
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                       WITHDRAW TRANSACTION                              │
   │  tx_hash: "ghi789"                                                      │
   │  metadata: { "event": "withdraw", "milestone": "m1" }                   │
   │            (NO project_id!)                                             │
   │                                                                         │
   │  inputs:                                                                │
   │    [0] ← UTXO₂ (spending def456:0)                                     │
   │                                                                         │
   │  ──► find_project_from_inputs("ghi789"):                                │
   │      1. Get inputs: [(def456, 0)]                                      │
   │      2. Lookup treasury.utxo_history WHERE tx_hash="def456" AND index=0│
   │      3. Found! project_db_id = 1                                       │
   │      4. UPDATE milestones SET withdrawn=TRUE WHERE milestone_id='m1'   │
   └─────────────────────────────────────────────────────────────────────────┘
```

#### Disambiguation when a tx pulls inputs from multiple project chains

A single milestone-level tx can include fee/collateral inputs from a sibling
project's UTXO chain. `find_project_from_inputs` collects every candidate
`project_db_id`, then scores each candidate by how many of the tx's metadata
`body.milestones` keys (collected via `collect_milestone_id_hints` in
`event_processor.rs`) match milestones stored for that project. The
best-scoring candidate wins; ties fall back to the first input.

#### Cold replay — when chain tracing can't reconstruct the link

The Postgres triggers installed by `install_utxo_history_triggers`
(`api/src/services/sync.rs`) capture every script-address UTXO into
`treasury.utxo_history` synchronously with YACI Store's INSERT, so the
chain-trace input is always available regardless of pruning — *provided
the triggers were armed before the relevant blocks were ingested*. If a
fresh local sync runs against a database where YACI Store has already
pruned UTXOs from before the triggers were installed, the chain trace can
return `None`; the event is still recorded in `treasury.events` (with
`project_db_id = NULL`) so nothing is silently dropped, but milestone
state flags can't be updated. See
[`docs/known-issues.md` `KI-CR-01`](known-issues.md) and `KI-UTX-01`.

### Stage 6: API Request Flow

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                           API REQUEST FLOW                                   │
└──────────────────────────────────────────────────────────────────────────────┘

   Client Request: GET /api/v1/projects/EC-0008-25
                                      │
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                         AXUM ROUTER                                      │
   │                                                                          │
   │   .nest("/api/v1", routes::v1::router())                                │
   │     → /projects/:project_id → get_project()                             │
   └─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                  routes/v1/projects.rs                                   │
   │                                                                          │
   │   pub async fn get_project(                                             │
   │       Extension(pool): Extension<PgPool>,                               │
   │       Path(project_id): Path<String>,                                   │
   │   ) -> Result<Json<ApiResponse<ProjectDetail>>, ApiError>               │
   └─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ SQL Query
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                        PostgreSQL                                        │
   │                                                                          │
   │   SELECT * FROM treasury.v_projects_summary                             │
   │   WHERE project_id = 'EC-0008-25'                                       │
   └─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                      JSON Response (v1 envelope)                         │
   │                                                                          │
   │   {                                                                     │
   │     "data": {                                                           │
   │       "project_id": "EC-0008-25",                                       │
   │       "project_name": "Community Hub Development",                      │
   │       "status": "active",                                               │
   │       "initial_amount_lovelace": 1000000000000,                         │
   │       "milestones_summary": { "total": 5, "withdrawn": 2 },             │
   │       "financials": {                                                   │
   │         "total_allocated_lovelace": 1000000000000,                      │
   │         "total_withdrawn_lovelace": 400000000000,                       │
   │         "withdrawal_percentage": 40.0                                   │
   │       }                                                                 │
   │     },                                                                  │
   │     "meta": { "timestamp": "2026-01-28T10:30:00Z" }                     │
   │   }                                                                     │
   └─────────────────────────────────────────────────────────────────────────┘
```

## Database Schema Relationships

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         TREASURY SCHEMA (treasury.*)                         │
└──────────────────────────────────────────────────────────────────────────────┘

   ┌─────────────────────┐    ┌─────────────────────┐
   │ treasury_contracts  │    │   vendor_contracts  │  (Singleton PSSC row)
   ├─────────────────────┤    ├─────────────────────┤
   │ id (PK)             │    │ id (PK)             │
   │ contract_instance   │◄─┐ │ treasury_id (FK)    │─┐
   │ stake_credential    │  │ │ address (PSSC, uniq)│ │
   │ publish_tx_hash     │  │ │ stake_credential    │ │
   │ initialized_at      │  │ └─────────────────────┘ │
   └─────────────────────┘  │                         │
            │               │                         │
            │ 1:N           │                         │
            ▼               │                         │
   ┌─────────────────────┐  │   ┌─────────────────────┐
   │      projects       │  │   │      events         │
   ├─────────────────────┤  │   ├─────────────────────┤
   │ id (PK)             │◄─┼───│ project_db_id       │
   │ treasury_id (FK)    │──┘   │ treasury_id (FK)    │
   │ project_id (unique) │      │ milestone_id (FK)   │─┐
   │ project_name        │      │ tx_hash (unique)    │ │
   │ vendor_address      │      │ event_type          │ │
   │ status              │      │ slot                │ │
   │ contract_address    │      │ destination (JSONB) │ │
   │ vendor_payment_*    │      │ metadata (JSONB)    │ │
   └─────────────────────┘      └─────────────────────┘ │
            │                                           │
            │ 1:N                                       │
            ▼                                           │
   ┌─────────────────────┐                              │
   │     milestones      │◄─────────────────────────────┘
   ├─────────────────────┤
   │ id (PK)             │
   │ project_db_id       │
   │ milestone_id        │
   │ label               │
   │ amount_lovelace     │
   │ time_limit          │
   │ withdrawn           │
   │ evidence_provided   │
   │ paused              │
   │ archived            │
   │ withdraw_tx_hash    │
   │ complete_tx_hash    │
   │ superseded_by       │
   └─────────────────────┘

   ┌─────────────────────┐
   │    utxo_history     │  (Trigger-captured UTXO history at script addresses)
   ├─────────────────────┤
   │ tx_hash             │
   │ output_index        │
   │ project_db_id       │
   │ address             │
   │ lovelace_amount     │
   │ inline_datum_cbor   │
   │ spent               │
   │ spent_tx_hash       │
   └─────────────────────┘


┌──────────────────────────────────────────────────────────────────────────────┐
│                        YACI_STORE SCHEMA (yaci_store.*)                      │
└──────────────────────────────────────────────────────────────────────────────┘

   ┌─────────────────────┐         ┌─────────────────────┐
   │       block         │         │   address_utxo      │
   ├─────────────────────┤         ├─────────────────────┤
   │ hash (PK)           │         │ tx_hash             │
   │ number              │         │ output_index        │
   │ slot                │◄────────│ slot                │
   │ block_time          │         │ owner_addr          │
   │ tx_count            │         │ lovelace_amount     │
   └─────────────────────┘         │ owner_stake_cred    │
            │                      └─────────────────────┘
            │
            ▼
   ┌─────────────────────┐         ┌─────────────────────┐
   │    transaction      │         │transaction_metadata │
   ├─────────────────────┤         ├─────────────────────┤
   │ tx_hash (PK)        │◄────────│ tx_hash             │
   │ block               │         │ slot                │
   │ slot                │         │ label               │
   │ fee                 │         │ body (JSONB)        │
   │ inputs (JSONB)      │         └─────────────────────┘
   │ outputs (JSONB)     │
   └─────────────────────┘
            │
            ▼
   ┌─────────────────────┐
   │      tx_input       │
   ├─────────────────────┤
   │ tx_hash             │
   │ output_index        │
   │ spent_tx_hash       │
   └─────────────────────┘
```

## Storage Optimization

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                        STORAGE OPTIMIZATION LAYERS                           │
└──────────────────────────────────────────────────────────────────────────────┘

   FULL CARDANO BLOCKCHAIN
   ┌────────────────────────────────────────────────────────────────────────┐
   │  ~100+ GB of data                                                      │
   │  • All blocks, transactions, UTXOs, scripts, metadata, etc.           │
   └────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ YACI Store Plugin Filters
                                      ▼
   FILTERED YACI_STORE DATA
   ┌────────────────────────────────────────────────────────────────────────┐
   │  ~4 GB of data (95%+ reduction)                                       │
   │                                                                        │
   │  ✓ Only treasury stake credential UTXOs                               │
   │  ✓ Only label 1694 metadata                                           │
   │  ✗ No CBOR storage (save-cbor=false)                                  │
   │  ✗ No witness data (save-witness=false)                               │
   │  ✗ Spent UTXOs pruned (pruning-enabled=true)                          │
   └────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ API Event Processing
                                      ▼
   NORMALIZED TREASURY DATA
   ┌────────────────────────────────────────────────────────────────────────┐
   │  ~2 MB of data                                                        │
   │                                                                        │
   │  • Structured project/milestone data                                  │
   │  • Event audit log                                                    │
   │  • UTXO tracking for chain analysis                                   │
   └────────────────────────────────────────────────────────────────────────┘


   Configuration (application.properties):
   ┌────────────────────────────────────────────────────────────────────────┐
   │  # Disable unnecessary storage                                        │
   │  store.blocks.save-cbor=false                                         │
   │  store.transaction.save-cbor=false                                    │
   │  store.transaction.save-witness=false                                 │
   │                                                                        │
   │  # Enable UTXO pruning                                                │
   │  store.utxo.pruning-enabled=true                                      │
   │  store.utxo.pruning-safe-blocks=2160                                  │
   └────────────────────────────────────────────────────────────────────────┘
```
