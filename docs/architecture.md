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
│   │  • transaction              │    │  • vendor_contracts         │           │
│   │  • address_utxo             │    │  • milestones               │           │
│   │  • transaction_metadata     │    │  • events                   │           │
│   │  • tx_input                 │    │  • utxos                    │           │
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
│   │ publish │  │initialize│  │    fund    │  │ complete │  │ disburse │    │
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
│   treasury_contracts     vendor_contracts      milestones        events      │
│   ┌───────────────┐     ┌───────────────┐    ┌───────────┐    ┌──────────┐  │
│   │ id            │     │ id            │    │ id        │    │ id       │  │
│   │ instance      │◄────│ treasury_id   │◄───│ vendor_id │    │ tx_hash  │  │
│   │ name          │     │ project_id    │    │ label     │    │ event    │  │
│   │ publish_tx    │     │ project_name  │    │ status    │    │ metadata │  │
│   └───────────────┘     │ status        │    │ amount    │    └──────────┘  │
│                         └───────────────┘    └───────────┘                   │
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
   │     "vendor": { "name": "Acme Corp" },                                │
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
   │  2. INSERT vendor_contracts                                           │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.vendor_contracts                        │  │
   │     │   (project_id, project_name, vendor_name, ...)               │  │
   │     │ VALUES ('project-001', 'My Project', 'Acme Corp', ...)       │  │
   │     └──────────────────────────────────────────────────────────────┘  │
   │                                      │                                 │
   │                                      ▼                                 │
   │  3. INSERT milestones (for each milestone in array)                   │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.milestones                              │  │
   │     │   (vendor_contract_id, milestone_id, label, amount, status)  │  │
   │     │ VALUES (1, 'm1', 'Phase 1', 1000000, 'pending')              │  │
   │     │ VALUES (1, 'm2', 'Phase 2', 2000000, 'pending')              │  │
   │     └──────────────────────────────────────────────────────────────┘  │
   │                                      │                                 │
   │                                      ▼                                 │
   │  4. INSERT event record                                               │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.events                                  │  │
   │     │   (tx_hash, event_type, vendor_contract_id, metadata)        │  │
   │     └──────────────────────────────────────────────────────────────┘  │
   │                                      │                                 │
   │                                      ▼                                 │
   │  5. Track UTXOs for future event lookups                              │
   │     ┌──────────────────────────────────────────────────────────────┐  │
   │     │ INSERT INTO treasury.utxos (tx_hash, output_index,           │  │
   │     │                             vendor_contract_id, spent)       │  │
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
   │  ──► Record in treasury.utxos:                                         │
   │      (tx_hash="abc123", output_index=0, vendor_contract_id=1)          │
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
   │  ──► find_vendor_contract_from_inputs("def456"):                       │
   │      1. Get inputs: [(abc123, 0)]                                      │
   │      2. Lookup treasury.utxos WHERE tx_hash="abc123" AND index=0       │
   │      3. Found! vendor_contract_id = 1                                  │
   │      4. Mark UTXO₁ as spent, record UTXO₂ with vendor_contract_id=1   │
   └─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ UTXO₂ is spent
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                       DISBURSE TRANSACTION                              │
   │  tx_hash: "ghi789"                                                      │
   │  metadata: { "event": "disburse" }                                      │
   │            (NO project_id!)                                             │
   │                                                                         │
   │  inputs:                                                                │
   │    [0] ← UTXO₂ (spending def456:0)                                     │
   │                                                                         │
   │  ──► find_vendor_contract_from_inputs("ghi789"):                       │
   │      1. Get inputs: [(def456, 0)]                                      │
   │      2. Lookup treasury.utxos WHERE tx_hash="def456" AND index=0       │
   │      3. Found! vendor_contract_id = 1                                  │
   └─────────────────────────────────────────────────────────────────────────┘
```

### Stage 6: API Request Flow

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                           API REQUEST FLOW                                   │
└──────────────────────────────────────────────────────────────────────────────┘

   Client Request: GET /api/v1/vendor-contracts/EC-0008-25
                                      │
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                         AXUM ROUTER                                      │
   │                                                                          │
   │   .nest("/api/v1", routes::v1::router())                                │
   │     → /vendor-contracts/:project_id → get_vendor_contract()             │
   └─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                  routes/v1/vendor_contracts.rs                           │
   │                                                                          │
   │   pub async fn get_vendor_contract(                                     │
   │       Extension(pool): Extension<PgPool>,                               │
   │       Path(project_id): Path<String>,                                   │
   │   ) -> Result<Json<ApiResponse<VendorContractDetail>>, StatusCode>      │
   └─────────────────────────────────────────────────────────────────────────┘
                                      │
                                      │ SQL Query
                                      ▼
   ┌─────────────────────────────────────────────────────────────────────────┐
   │                        PostgreSQL                                        │
   │                                                                          │
   │   SELECT * FROM treasury.v_vendor_contracts_summary                     │
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
   │       "vendor_name": "Acme Blockchain Solutions",                       │
   │       "status": "active",                                               │
   │       "initial_amount_lovelace": 1000000000000,                         │
   │       "initial_amount_ada": 1000000.0,                                  │
   │       "milestones_summary": { "total": 5, "disbursed": 2 },             │
   │       "financials": {                                                   │
   │         "total_allocated_ada": 1000000.0,                               │
   │         "total_disbursed_ada": 400000.0,                                │
   │         "disbursement_percentage": 40.0                                 │
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

   ┌─────────────────────┐
   │ treasury_contracts  │
   ├─────────────────────┤
   │ id (PK)             │
   │ contract_instance   │◄─────────────────────────────────────────────┐
   │ name                │                                              │
   │ publish_tx_hash     │                                              │
   │ initialized_at      │                                              │
   └─────────────────────┘                                              │
            │                                                           │
            │ 1:N                                                       │
            ▼                                                           │
   ┌─────────────────────┐         ┌─────────────────────┐             │
   │  vendor_contracts   │         │      events         │             │
   ├─────────────────────┤         ├─────────────────────┤             │
   │ id (PK)             │◄────────│ vendor_contract_id  │             │
   │ treasury_id (FK)    │─────────│ treasury_id (FK)    │─────────────┘
   │ project_id (unique) │         │ milestone_id (FK)   │─────┐
   │ project_name        │         │ tx_hash (unique)    │     │
   │ vendor_name         │         │ event_type          │     │
   │ status              │         │ slot                │     │
   │ contract_address    │         │ metadata (JSONB)    │     │
   └─────────────────────┘         └─────────────────────┘     │
            │                                                   │
            │ 1:N                                               │
            ▼                                                   │
   ┌─────────────────────┐                                     │
   │     milestones      │◄────────────────────────────────────┘
   ├─────────────────────┤
   │ id (PK)             │
   │ vendor_contract_id  │
   │ milestone_id        │
   │ label               │
   │ status              │
   │ amount_lovelace     │
   │ complete_tx_hash    │
   │ disburse_tx_hash    │
   └─────────────────────┘

   ┌─────────────────────┐
   │       utxos         │  (Tracks UTXO chain for event linking)
   ├─────────────────────┤
   │ tx_hash (PK)        │
   │ output_index (PK)   │
   │ vendor_contract_id  │
   │ address             │
   │ lovelace_amount     │
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
