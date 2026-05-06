# Administration API Backend

Rust-based REST API for querying Cardano treasury fund tracking data. Built with Axum framework, SQLx for PostgreSQL, and utoipa for OpenAPI documentation.

## Features

- RESTful API with OpenAPI/Swagger documentation
- Consistent response envelopes with pagination
- All amounts in lovelace (1 ADA = 1,000,000 lovelace)
- Raw metadata AND parsed/normalized data
- Background sync service for real-time data

## Quick Start

```bash
# Start with Docker Compose (recommended)
cd ..
./dev.sh start

# API available at http://localhost:8080
# Swagger UI at http://localhost:8080/docs
```

## API Reference

Base URL: `http://localhost:8080`

Interactive documentation: `http://localhost:8080/docs`

### Response Format

All responses use a consistent envelope:

```json
{
  "data": { ... },
  "pagination": {
    "page": 1,
    "limit": 50,
    "total_count": 150,
    "has_next": true
  },
  "meta": {
    "timestamp": "2026-01-28T10:30:00Z"
  }
}
```

- `data`: The response payload
- `pagination`: Only present for paginated endpoints
- `meta.timestamp`: When the response was generated

### Amount Fields

All monetary amounts are in lovelace (1 ADA = 1,000,000 lovelace):

```json
{
  "initial_amount_lovelace": 1000000000000
}
```

---

## Endpoints

### Health Check

#### `GET /health`

Returns the health status of the API.

**Response:** `OK`

---

### Status

#### `GET /api/v1/status`

Get API status and sync information. Three time domains are surfaced separately:
`database.checked_at` (server-side), `sync.heartbeat` (server-side, last sync poll),
`sync.last_event_processed` (on-chain block time of most recent processed TOM event),
and `chain.indexer_time` (on-chain block time of YACI Store's tip).

**Response:**
```json
{
  "data": {
    "api_version": "2.0.0",
    "database": {
      "connected": true,
      "checked_at": "2026-05-01T10:30:00Z"
    },
    "sync": {
      "heartbeat": "2026-05-01T10:29:55Z",
      "last_event_processed": { "unix": 1777623100, "iso": "2026-05-01T08:11:40Z" }
    },
    "chain": {
      "indexer_block": 12296746,
      "indexer_slot": 163964156,
      "indexer_time": { "unix": 1777623200, "iso": "2026-05-01T08:13:20Z" }
    },
    "totals": {
      "events": 411,
      "projects": 42,
      "events_by_type": { "fund": 42, "complete": 189, "withdraw": 129, "pause": 63, "resume": 32 }
    }
  },
  "meta": {
    "timestamp": "2026-05-01T10:30:00Z"
  }
}
```

---

### Treasury

#### `GET /api/v1/treasury`

Get treasury contract details with statistics and financials.

**Response:**
```json
{
  "data": {
    "id": 1,
    "contract_instance": "9e65e4ed7d6fd86fc4827d2b45da6d2c601fb920e8bfd794b8ecc619",
    "contract_address": "addr1xxzc8pt7fgf0lc0x7eq6z7z6puhsxmzktna7dluahrj6g6...",
    "stake_credential": "8583857e4a12ffe1e6f641a1785a0f2f036c565cfbe6ff9db8e5a469",
    "status": "active",
    "publish_tx_hash": "abc123...",
    "publish_time": { "unix": 1704067200, "iso": "2024-01-01T00:00:00Z" },
    "initialized_tx_hash": "def456...",
    "initialized_at": { "unix": 1704067300, "iso": "2024-01-01T00:01:40Z" },
    "permissions": { ... },
    "statistics": {
      "project_count": 42,
      "active_contracts": 35,
      "completed_contracts": 6,
      "cancelled_contracts": 1,
      "total_events": 45,
      "utxo_count": 12,
      "last_event_time": { "unix": 1704153600, "iso": "2024-01-02T00:00:00Z" }
    },
    "financials": {
      "balance_lovelace": 264568247000000
    },
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-15T12:00:00Z"
  },
  "meta": { ... }
}
```

#### `GET /api/v1/treasury/utxos`

Get all unspent UTXOs at the treasury contract address.

**Response:**
```json
{
  "data": [
    {
      "tx_hash": "abc123...",
      "output_index": 0,
      "address": "addr1x...",
      "address_type": "treasury",
      "lovelace_amount": 100000000000,
      "slot": 163964156,
      "block_number": 12296746
    }
  ],
  "meta": { ... }
}
```

#### `GET /api/v1/treasury/events`

Get treasury-level events (publish, initialize, sweep, reorganize).

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number (1-indexed) |
| `limit` | integer | 50 | Results per page (max: 100) |

---

### Vendor Contract (singleton PSSC)

#### `GET /api/v1/vendor-contract`

Get the shared vendor contract — the singleton on-chain script address every project sits at, plus a quick rollup of the projects bound to it.

**Response:**
```json
{
  "data": {
    "address": "addr1x...",
    "stake_credential": "8583857e...",
    "projects": {
      "total": 42,
      "by_status": { "active": 35, "completed": 6, "cancelled": 1 }
    }
  },
  "meta": { ... }
}
```

**Errors:**
- `404 Not Found` - Vendor contract not yet known (first fund event has not been processed)

---

#### `GET /api/v1/vendor-contract/utxos`

List currently-unspent UTxOs at the shared vendor contract, each row labeled with its owning project. Lets you enumerate every live PSSC output in one call instead of fanning out across every project.

"Currently unspent" is sourced from `yaci_store.address_utxo` with an anti-join against `yaci_store.tx_input` (same approach as `/projects/:id/utxos` and `/treasury/utxos`).

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number (1-indexed) |
| `limit` | integer | 50 | Results per page (max: 100) |

**Example:**
```bash
curl "http://localhost:8080/api/v1/vendor-contract/utxos?limit=10"
```

**Response:**
```json
{
  "data": [
    {
      "tx_hash": "cb923b75...",
      "output_index": 0,
      "address": "addr1x...",
      "lovelace_amount": 79500000000,
      "slot": 186056809,
      "block_number": 13361422,
      "project_db_id": 8,
      "project_id": "EG-0001-25",
      "project_name": "AdaStat.net Cardano blockchain explorer",
      "project_status": "active"
    }
  ],
  "pagination": { "page": 1, "limit": 10, "total_count": 33, "has_next": true },
  "meta": { ... }
}
```

**Errors:**
- `404 Not Found` - Vendor contract not yet known (first fund event has not been processed)

---

### Projects

#### `GET /api/v1/projects`

List all projects (one per `fund` event) with pagination and filtering.

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number (1-indexed) |
| `limit` | integer | 50 | Results per page (max: 100) |
| `status` | string | - | Filter by status: `active`, `paused`, `completed`, `cancelled` |
| `search` | string | - | Search in project_id, project_name, description |
| `sort` | string | `fund_time` | Sort field: `fund_time`, `project_id`, `project_name`, `initial_amount` |
| `order` | string | `desc` | Sort order: `asc`, `desc` |
| `from_time` | integer | - | Filter by fund time (Unix timestamp, from) |
| `to_time` | integer | - | Filter by fund time (Unix timestamp, to) |

**Example:**
```bash
curl "http://localhost:8080/api/v1/projects?status=active&search=community&limit=10"
```

**Response:**
```json
{
  "data": [
    {
      "id": 1,
      "project_id": "EC-0008-25",
      "project_name": "Community Hub Development",
      "description": "Building decentralized community infrastructure",
      "vendor_address": "addr1q...",
      "contract_address": "addr1x...",
      "status": "active",
      "fund_tx_hash": "abc123...",
      "fund_time": { "unix": 1704067200, "iso": "2024-01-01T00:00:00Z" },
      "initial_amount_lovelace": 1000000000000,
      "milestones_summary": {
        "total": 5,
        "pending": 2,
        "completed": 2,
        "withdrawn": 1,
        "paused": 0
      },
      "financials": {
        "total_allocated_lovelace": 1000000000000,
        "total_withdrawn_lovelace": 400000000000,
        "current_balance_lovelace": 600000000000,
        "withdrawal_percentage": 40.0,
        "utxo_count": 3
      },
      "treasury": {
        "contract_instance": "9e65e4ed..."
      },
      "last_event_time": { "unix": 1704153600, "iso": "2024-01-02T00:00:00Z" },
      "event_count": 8
    }
  ],
  "pagination": {
    "page": 1,
    "limit": 10,
    "total_count": 5,
    "has_next": false
  },
  "meta": { ... }
}
```

#### `GET /api/v1/projects/:project_id`

Get detailed information about a specific project.

**Path Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `project_id` | string | Project identifier (e.g., "EC-0008-25") |

**Response:** Same as list item but with additional fields:
- `other_identifiers`: Related project IDs
- `vendor_payment_key_hash`: Vendor payment key hash from inline datum
- `current_utxos`: Array of `{ tx_hash, output_index, lovelace_amount, slot }` for the project's currently-unspent outputs at the vendor contract. Empty when fully withdrawn. Sum equals `financials.current_balance_lovelace`.
- `created_at`, `updated_at`: Timestamps

**Errors:**
- `404 Not Found` - Project not found

#### `GET /api/v1/projects/:project_id/milestones`

Get all (non-archived) milestones for a specific project. Paginated.

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number |
| `limit` | integer | 50 | Results per page (max: 100) |

**Response:**
```json
{
  "data": [
    {
      "id": 1,
      "milestone_id": "m-0",
      "milestone_order": 1,
      "label": "Phase 1: Research",
      "description": "Complete market research and requirements gathering",
      "acceptance_criteria": "Deliver research report",
      "amount_lovelace": 200000000000,
      "time_limit": 1704240000000,
      "withdrawn": true,
      "evidence_provided": true,
      "paused": false,
      "archived": false,
      "completion": {
        "tx_hash": "abc123...",
        "time": { "unix": 1704067200, "iso": "2024-01-01T00:00:00Z" },
        "description": "Research completed successfully",
        "evidence": [...]
      },
      "withdrawal": {
        "tx_hash": "def456...",
        "time": { "unix": 1704153600, "iso": "2024-01-02T00:00:00Z" },
        "amount_lovelace": 200000000000
      },
      "archive_info": null,
      "pause_history": null,
      "project": {
        "project_id": "EC-0008-25",
        "project_name": "Community Hub Development"
      }
    }
  ],
  "pagination": { ... },
  "meta": { ... }
}
```

`pause_history` is non-null when at least one pause/resume event has been recorded for the milestone. It carries `currently_paused`, `last_pause_tx_hash` / `last_pause_time` and `last_resume_tx_hash` / `last_resume_time`.

#### `GET /api/v1/projects/:project_id/events`

Get event history for a specific project.

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number |
| `limit` | integer | 50 | Results per page |
| `type` | string | - | Filter by event type |

#### `GET /api/v1/projects/:project_id/utxos`

Get current (unspent) UTXOs for a specific project. Paginated.

---

### Milestones

#### `GET /api/v1/milestones`

List all milestones across all projects.

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number |
| `limit` | integer | 50 | Results per page |
| `withdrawn` | boolean | - | Filter by withdrawn status |
| `evidence_provided` | boolean | - | Filter by evidence provided status |
| `archived` | boolean | false | Filter by archived status (defaults to false) |
| `project_id` | string | - | Filter by project ID |
| `sort` | string | - | Sort field: `milestone_order`, `complete_time`, `withdraw_time`, `amount` |
| `from_time` | integer | - | Filter by milestone time (Unix timestamp, from). Matches whichever of `complete_time` or `withdraw_time` is set on the row. |
| `to_time` | integer | - | Filter by milestone time (Unix timestamp, to). |

#### `GET /api/v1/milestones/:project_id`

List milestones for a specific project (paginated). Convenience endpoint mirroring `/api/v1/projects/{project_id}/milestones`, served under the `/milestones/` root.

**Path Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `project_id` | string | Project identifier (e.g., "EC-0008-25") |

#### `GET /api/v1/milestones/by-id/:id`

Get a specific milestone by integer database ID. The integer ID is rarely useful to clients; prefer the project-scoped lookup above.

**Path Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `id` | integer | Milestone database ID |

---

### Events

#### `GET /api/v1/events`

List all events with full context.

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number |
| `limit` | integer | 50 | Results per page |
| `type` | string | - | Filter by event type |
| `project_id` | string | - | Filter by project ID |
| `from_time` | integer | - | Filter by time (Unix timestamp, from) |
| `to_time` | integer | - | Filter by time (Unix timestamp, to) |
| `q` | string | - | Full-text search across `reason`, `destination`, and raw `metadata` (case-insensitive substring) |

**Response:**
```json
{
  "data": [
    {
      "id": 1,
      "tx_hash": "abc123...",
      "slot": 163964156,
      "block_number": 12296746,
      "block_time": { "unix": 1704067200, "iso": "2024-01-01T00:00:00Z" },
      "event_type": "fund",
      "amount_lovelace": 1000000000000,
      "reason": null,
      "destination": null,
      "treasury": {
        "contract_instance": "9e65e4ed..."
      },
      "project": {
        "project_id": "EC-0008-25",
        "project_name": "Community Hub Development",
        "contract_address": "addr1x..."
      },
      "milestone": null,
      "metadata_raw": { ... },
      "created_at": "2024-01-01T00:00:00Z"
    }
  ],
  "pagination": { ... },
  "meta": { ... }
}
```

`destination` is a JSONB `{label, details}` object preserved as-is from the TOM metadata; populated on `disburse` events only.

#### `GET /api/v1/events/recent`

Get recent events for activity feeds.

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `hours` | integer | 24 | Hours to look back (max: 168 = 1 week) |
| `limit` | integer | 50 | Maximum events to return |
| `type` | string | - | Filter by event type |

#### `GET /api/v1/events/:tx_hash`

Get a specific event by transaction hash.

**Path Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `tx_hash` | string | Transaction hash (64 hex characters) |

---

### Statistics

#### `GET /api/v1/statistics`

Get comprehensive statistics across all data.

**Response:**
```json
{
  "data": {
    "treasury": {
      "total_count": 1,
      "active_count": 1,
      "disbursed_count": 3
    },
    "vendor_contracts": {
      "total_count": 1,
      "address": "addr1x...",
      "project_count": 42,
      "utxo_history_count": 1235,
      "unspent_utxo_count": 449,
      "current_balance_lovelace": 600000000000
    },
    "projects": {
      "total_count": 42,
      "active_count": 35,
      "completed_count": 6,
      "paused_count": 0,
      "cancelled_count": 1
    },
    "milestones": {
      "total_count": 364,
      "pending_count": 100,
      "completed_count": 60,
      "withdrawn_count": 204
    },
    "events": {
      "on_chain_count": 411,
      "processed_count": 411,
      "by_type": {
        "fund": 42,
        "complete": 189,
        "withdraw": 129,
        "pause": 63,
        "resume": 32
      }
    },
    "financials": {
      "total_allocated_lovelace": 5000000000000,
      "total_withdrawn_lovelace": 2000000000000,
      "current_balance_lovelace": 3000000000000
    },
    "sync": {
      "last_slot": 163964156,
      "last_block": 12296746,
      "last_updated": "2026-05-01T08:11:40Z"
    }
  },
  "meta": { ... }
}
```

`vendor_contracts` is the singleton-PSSC rollup (see `GET /api/v1/vendor-contract`); `projects` counts rows in `treasury.projects`.

---

## Event Types

The API tracks the following Treasury Oversight Metadata (TOM) events:

| Event | Description |
|-------|-------------|
| `publish` | Publish a treasury contract |
| `initialize` | Initialize a treasury contract |
| `fund` | Fund a vendor contract from treasury |
| `complete` | Submit evidence of milestone completion |
| `disburse` | Disburse funds from treasury (treasury-level) |
| `withdraw` | Vendor withdraws matured milestone funds (milestone-level) |
| `pause` | Pause a contract |
| `resume` | Resume a paused contract |
| `modify` | Modify contract parameters |
| `cancel` | Cancel a contract |
| `sweep` | Sweep remaining funds |
| `reorganize` | Reorganize treasury funds |

For per-event field mappings (which JSON path becomes which DB column) see
[`docs/event-processing.md`](../docs/event-processing.md). For the catalog of
known data-quality holes (NULL fields, on-chain inconsistencies, sync-loop
quirks) see [`docs/known-issues.md`](../docs/known-issues.md).

---

## Event Processing Pipeline

The API runs a background sync task (`api/src/services/sync.rs::run_sync_loop`)
that drives event ingestion. The pipeline has four stages:

1. **Pre-fetch UTXOs** — `EventProcessor::pre_fetch_utxos`
   (`api/src/services/event_processor.rs`) batches the tx_hashes of
   pending TOM events and copies their outputs and inputs from
   `yaci_store.address_utxo` into `treasury.utxo_history`. This is a
   defensive backstop on top of the Postgres triggers (`install_utxo_history_triggers`
   in `api/src/services/sync.rs`) that capture every script-address UTXO
   into `treasury.utxo_history` synchronously with YACI Store's INSERT.
2. **Dispatch** — `process_event` (`event_processor.rs`) reads
   `body.body.event` and delegates to a per-event handler. Treasury-level
   events (`publish`, `initialize`, `disburse`, `sweep`, `reorganize`) write
   to `treasury_contracts` + `events`; project-level events write to
   `projects` + `milestones` + `events`.
3. **Project resolution** — milestone-level events
   (`complete`/`withdraw`/`pause`/`resume`) take their `project_db_id`
   from `body.identifier` when present, otherwise from
   `find_project_from_inputs`, which traces input UTXOs back to the seed
   planted by the project's `fund` event. When multiple project chains
   feed a single tx (sibling-project fee inputs, etc.) the trace
   disambiguates by scoring candidate projects against the metadata's
   milestone keys (`collect_milestone_id_hints`).
4. **Insert** — `insert_event_full` writes one row per `tx_hash` into
   `treasury.events` with `ON CONFLICT (tx_hash) DO UPDATE`, preserving
   idempotency. Events are recorded even when the chain trace fails
   (`project_db_id IS NULL`) so nothing is silently dropped.

In addition to the incremental loop, a separate `tokio::spawn` task runs
`sync_all_events` every 10 minutes as an idempotent backfill — any event
that wedged the incremental loop (e.g. a postgres restart mid-batch) is
recovered by the next full re-sync via the `ON CONFLICT DO UPDATE` chain.
See `KI-SY-02` in `docs/known-issues.md`.

Datum parsing (milestone amounts, time limits, paused flags, vendor payment
key hash) lives in `api/src/parsers/datum.rs`; address parsing
(stake-credential extraction from bech32) lives in
`api/src/parsers/address.rs`.

For the SQL queries that surface where this pipeline produces NULLs in
practice, see the repro queries in
[`docs/known-issues.md`](../docs/known-issues.md).

---

## Error Responses

All endpoints return standard HTTP status codes:

| Status Code | Description |
|-------------|-------------|
| `200 OK` | Request successful |
| `404 Not Found` | Resource not found |
| `500 Internal Server Error` | Database or server error |

---

## Development

### Prerequisites

- Rust 1.75+ (install via https://rustup.rs/)
- PostgreSQL database (use docker-compose postgres service)

### Local Setup

1. Install dependencies:
```bash
cargo build
```

2. Set up environment variables:
```bash
export DATABASE_URL=postgresql://postgres:postgres@localhost:5433/administration_data
```

3. Run the API:
```bash
cargo run
```

The API will start on `http://localhost:8080` with Swagger UI at `/docs`.

### Building for Production

```bash
cargo build --release
```

### Docker

Build the Docker image:
```bash
docker build -t administration-api .
```

Run the container:
```bash
docker run -p 8080:8080 \
  -e DATABASE_URL=postgresql://postgres:postgres@postgres:5432/administration_data \
  administration-api
```

---

## Database Schema

The API queries the `treasury` schema:

| Table | Description |
|-------|-------------|
| `treasury.treasury_contracts` | Treasury reserve contracts (TRSC) |
| `treasury.vendor_contracts` | Singleton row for the shared PSSC script address |
| `treasury.projects` | One row per `fund` event (the 42 active projects) |
| `treasury.milestones` | Project milestones; FK to `projects.id` via `project_db_id` |
| `treasury.events` | All TOM event audit log; FK to `projects.id` via `project_db_id`; `destination` is JSONB |
| `treasury.utxo_history` | Persistent UTXO history (Postgres-trigger captured) for chain trace + datum cache |
| `treasury.sync_status` | Sync progress tracking |

### Views

| View | Description |
|------|-------------|
| `v_treasury_summary` | Treasury with statistics and financials |
| `v_projects_summary` | Projects with milestone counts and financials |
| `v_events_with_context` | Events with treasury/project/milestone context |
| `v_recent_events` | Events with context, ordered by slot DESC |
| `v_financial_summary` | Allocated vs withdrawn vs remaining |
| `v_milestone_timeline` | Milestones with project context |
