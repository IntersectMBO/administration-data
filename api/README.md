# Administration API Backend

Rust-based REST API for querying Cardano treasury fund tracking data. Built with Axum framework, SQLx for PostgreSQL, and utoipa for OpenAPI documentation.

## Features

- RESTful API with OpenAPI/Swagger documentation
- Consistent response envelopes with pagination
- Both lovelace AND ADA amounts in responses
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

All monetary amounts include both representations:

```json
{
  "initial_amount_lovelace": 1000000000000,
  "initial_amount_ada": 1000000.0
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

Get API status and sync information.

**Response:**
```json
{
  "data": {
    "api_version": "1.0.0",
    "database_connected": true,
    "last_sync_slot": 163964156,
    "last_sync_block": 12296746,
    "last_sync_time": 1704067200,
    "total_events": 21,
    "total_vendor_contracts": 5
  },
  "meta": {
    "timestamp": "2026-01-28T10:30:00Z"
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
    "name": "CC Treasury",
    "status": "active",
    "publish_tx_hash": "abc123...",
    "publish_time": 1704067200,
    "initialized_tx_hash": "def456...",
    "initialized_at": 1704067300,
    "permissions": { ... },
    "statistics": {
      "vendor_contract_count": 10,
      "active_contracts": 8,
      "completed_contracts": 2,
      "cancelled_contracts": 0,
      "total_events": 45,
      "utxo_count": 12,
      "last_event_time": 1704153600
    },
    "financials": {
      "balance_lovelace": 264568247000000,
      "balance_ada": 264568247.0
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
      "ada_amount": 100000.0,
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

### Vendor Contracts

#### `GET /api/v1/vendor-contracts`

List all vendor contracts (projects) with pagination and filtering.

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number (1-indexed) |
| `limit` | integer | 50 | Results per page (max: 100) |
| `status` | string | - | Filter by status: `active`, `paused`, `completed`, `cancelled` |
| `search` | string | - | Search in project_id, project_name, description, vendor_name |
| `sort` | string | `fund_time` | Sort field: `fund_time`, `project_id`, `project_name`, `initial_amount` |
| `order` | string | `desc` | Sort order: `asc`, `desc` |
| `from_time` | integer | - | Filter by fund time (Unix timestamp, from) |
| `to_time` | integer | - | Filter by fund time (Unix timestamp, to) |

**Example:**
```bash
curl "http://localhost:8080/api/v1/vendor-contracts?status=active&search=community&limit=10"
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
      "vendor_name": "Acme Blockchain Solutions",
      "vendor_address": "addr1q...",
      "contract_url": "https://...",
      "contract_address": "addr1x...",
      "status": "active",
      "fund_tx_hash": "abc123...",
      "fund_time": 1704067200,
      "initial_amount_lovelace": 1000000000000,
      "initial_amount_ada": 1000000.0,
      "milestones_summary": {
        "total": 5,
        "pending": 2,
        "completed": 2,
        "disbursed": 1
      },
      "financials": {
        "total_allocated_lovelace": 1000000000000,
        "total_allocated_ada": 1000000.0,
        "total_disbursed_lovelace": 400000000000,
        "total_disbursed_ada": 400000.0,
        "current_balance_lovelace": 600000000000,
        "current_balance_ada": 600000.0,
        "disbursement_percentage": 40.0,
        "utxo_count": 3
      },
      "treasury": {
        "contract_instance": "9e65e4ed...",
        "name": "CC Treasury"
      },
      "last_event_time": 1704153600,
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

#### `GET /api/v1/vendor-contracts/:project_id`

Get detailed information about a specific vendor contract.

**Path Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `project_id` | string | Project identifier (e.g., "EC-0008-25") |

**Response:** Same as list item but with additional fields:
- `other_identifiers`: Related project IDs
- `created_at`, `updated_at`: Timestamps

**Errors:**
- `404 Not Found` - Vendor contract not found

#### `GET /api/v1/vendor-contracts/:project_id/milestones`

Get all milestones for a specific project.

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
      "amount_ada": 200000.0,
      "status": "disbursed",
      "completion": {
        "tx_hash": "abc123...",
        "time": 1704067200,
        "description": "Research completed successfully",
        "evidence": [...]
      },
      "disbursement": {
        "tx_hash": "def456...",
        "time": 1704153600,
        "amount_lovelace": 200000000000,
        "amount_ada": 200000.0
      },
      "project": {
        "project_id": "EC-0008-25",
        "project_name": "Community Hub Development"
      }
    }
  ],
  "meta": { ... }
}
```

#### `GET /api/v1/vendor-contracts/:project_id/events`

Get event history for a specific project.

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number |
| `limit` | integer | 50 | Results per page |
| `type` | string | - | Filter by event type |

#### `GET /api/v1/vendor-contracts/:project_id/utxos`

Get current (unspent) UTXOs for a specific project.

---

### Milestones

#### `GET /api/v1/milestones`

List all milestones across all projects.

**Query Parameters:**

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `page` | integer | 1 | Page number |
| `limit` | integer | 50 | Results per page |
| `status` | string | - | Filter by status: `pending`, `completed`, `disbursed` |
| `project_id` | string | - | Filter by project ID |
| `sort` | string | - | Sort field: `milestone_order`, `complete_time`, `disburse_time`, `amount` |

#### `GET /api/v1/milestones/:id`

Get a specific milestone by database ID.

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

**Response:**
```json
{
  "data": [
    {
      "id": 1,
      "tx_hash": "abc123...",
      "slot": 163964156,
      "block_number": 12296746,
      "block_time": 1704067200,
      "event_type": "fund",
      "amount_lovelace": 1000000000000,
      "amount_ada": 1000000.0,
      "reason": null,
      "destination": null,
      "treasury": {
        "contract_instance": "9e65e4ed...",
        "name": "CC Treasury"
      },
      "project": {
        "project_id": "EC-0008-25",
        "project_name": "Community Hub Development",
        "vendor_name": "Acme Blockchain Solutions",
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
      "active_count": 1
    },
    "projects": {
      "total_count": 10,
      "active_count": 8,
      "completed_count": 2,
      "paused_count": 0,
      "cancelled_count": 0
    },
    "milestones": {
      "total_count": 50,
      "pending_count": 20,
      "completed_count": 15,
      "disbursed_count": 15
    },
    "events": {
      "total_count": 45,
      "by_type": {
        "fund": 10,
        "complete": 15,
        "disburse": 15,
        "publish": 1,
        "initialize": 1,
        "pause": 2,
        "resume": 1
      }
    },
    "financials": {
      "total_allocated_lovelace": 5000000000000,
      "total_allocated_ada": 5000000.0,
      "total_disbursed_lovelace": 2000000000000,
      "total_disbursed_ada": 2000000.0,
      "current_balance_lovelace": 3000000000000,
      "current_balance_ada": 3000000.0
    },
    "sync": {
      "last_slot": 163964156,
      "last_block": 12296746,
      "last_updated": "2024-01-15T12:00:00Z"
    }
  },
  "meta": { ... }
}
```

---

## Event Types

The API tracks the following Treasury Oversight Metadata (TOM) events:

| Event | Description |
|-------|-------------|
| `publish` | Publish a treasury contract |
| `initialize` | Initialize a treasury contract |
| `fund` | Fund a vendor contract from treasury |
| `complete` | Mark a milestone as complete |
| `disburse` | Disburse funds for a completed milestone |
| `withdraw` | Withdraw funds |
| `pause` | Pause a contract |
| `resume` | Resume a paused contract |
| `modify` | Modify contract parameters |
| `cancel` | Cancel a contract |
| `sweep` | Sweep remaining funds |
| `reorganize` | Reorganize treasury funds |

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
| `treasury.vendor_contracts` | Vendor/project contracts (PSSC) |
| `treasury.milestones` | Project milestones |
| `treasury.events` | All TOM event audit log |
| `treasury.utxos` | UTXO tracking for event linking |
| `treasury.sync_status` | Sync progress tracking |

### Views

| View | Description |
|------|-------------|
| `v_treasury_summary` | Treasury with statistics and financials |
| `v_vendor_contracts_summary` | Projects with milestone counts and financials |
| `v_events_with_context` | Events with treasury/project/milestone context |
| `v_financial_summary` | Allocated vs disbursed vs remaining |
| `v_milestone_timeline` | Milestones with project context |
