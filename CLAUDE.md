# CLAUDE.md — Administration Data

## Project Overview

Indexes Cardano treasury governance data from the blockchain and exposes it via a REST API. Three components work together:

1. **YACI Store indexer** — Java-based blockchain indexer (black-box dependency) that reads from a Cardano node and writes raw data to PostgreSQL
2. **PostgreSQL** — stores both raw blockchain data (`yaci_store` schema) and normalized app data (`treasury` schema)
3. **Rust API** — syncs from YACI Store tables into treasury tables, then serves REST endpoints

Swagger docs are at `/docs` when the API is running.

## Architecture & Data Flow

```
Cardano Node → YACI Store indexer → PostgreSQL (yaci_store schema)
                                          ↓
                              Rust API sync service
                                          ↓
                              PostgreSQL (treasury schema)
                                          ↓
                                      REST API
```

- **`yaci_store` schema**: raw blockchain data, managed by YACI Store's Flyway migrations — never modify manually
- **`treasury` schema**: normalized app data, managed by `database/schema/treasury.sql` and init scripts
- The YACI Store plugin filter (`indexer/plugins/scripts/treasury-filter.mvel`) reduces stored data by ~95%

## Domain Context (TOM / Cardano Treasury)

This project implements the **Treasury Oversight Metadata (TOM)** standard, using CIP-100 metadata label **1694**.

### Contract Hierarchy
- **Treasury Contract (TRSC)** → at a unique script address, holds treasury reserve funds. Stored in `treasury.treasury_contracts`.
- **Vendor Contract (PSSC)** → **ONE shared script address for ALL projects** (not one per project). Stored in `treasury.vendor_contracts` (singleton row: `address`, `stake_credential`).
  - Each `fund` tx creates UTXOs at the shared PSSC address
  - UTXOs belong to specific projects, distinguished by inline datum, NOT by address
  - UTXO chain tracking (`find_project_from_inputs`) links events to projects by tracing spent inputs
- **Project** → one row per `fund` event (e.g. `EC-0008-25`). Stored in `treasury.projects`. Foreign-keyed from milestones, events, and UTXO history via `project_db_id`.
- **Milestones** → belong to a project

### Vendor Naming
- `vendor.name` does **not exist** in the TOM spec — code extracts it but always gets null
- `vendor.label` in the spec is the vendor's display name; in practice, real metadata puts the payment address here
- Vendor identity is typically embedded in the top-level `body.label` by convention (e.g., "Tastenkunst GmbH - Eternl Maintenance")

### Event Types
publish, initialize, fund, complete, disburse, withdraw, pause, resume, modify, cancel, sweep, reorganize

See [`docs/event-processing.md`](docs/event-processing.md) for detailed per-event field mappings, code extraction paths, DB writes, and known bugs.

### Financial Model
- All amounts are in **lovelace** (1 ADA = 1,000,000 lovelace)

### Milestone Lifecycle
Milestones use 4 independent boolean flags (not a linear status):
- **evidence_provided** — vendor submitted completion evidence via a `complete` transaction
- **withdrawn** — vendor withdrew payment via a `withdraw` transaction
- **paused** — oversight committee paused this milestone (from inline datum constructor 0→1)
- **archived** — milestone replaced by a `modify` event (old row preserved, new row created)

Additionally, each milestone has a **time_limit** (POSIXTime ms) from the inline UTXO datum.
Claimability is derived: time_limit < current time AND NOT withdrawn.

Archive model: on modify, existing row → archived=true, new row inserted. superseded_by FK links old → new.

**Disburse vs Withdraw**: Disburse is treasury-level (moves funds from treasury contract to any address).
Withdraw is milestone-level (vendor claims matured milestone funds from vendor contract). These are completely separate.

### Treasury Instance
The `TREASURY_INSTANCE` env var filters to a specific on-chain treasury. Changing it tracks a different treasury entirely.

## Development Setup

### Prerequisites
- Docker and docker-compose
- Rust toolchain (for native API development)

### Quick Start
```bash
./dev.sh start    # starts PostgreSQL + indexer + API
```

### Dev Script Commands
```bash
./dev.sh start    # start all services (API runs natively if Rust is installed)
./dev.sh stop     # stop all Docker services
./dev.sh restart  # restart Docker services
./dev.sh logs     # tail all logs (or: ./dev.sh logs indexer)
./dev.sh status   # show service status
./dev.sh build    # build Docker images
./dev.sh clean    # stop and remove all containers + volumes
```

### Native API Development
```bash
# With Docker DB already running:
cd api
DATABASE_URL="postgresql://postgres:postgres@localhost:5433/administration_data" cargo run
```

### Build & Test
```bash
cd api
cargo build --release
cargo check        # fast type-checking
cargo test         # run tests
```

### Environment Setup
Copy `.env.example` to `.env` and configure:
- `TREASURY_INSTANCE` — the on-chain treasury to track
- `STORE_CARDANO_SYNC_START_SLOT` / `STORE_CARDANO_SYNC_START_BLOCKHASH` — where to start syncing

## Port Mappings

| Service    | Host Port | Container Port |
|------------|-----------|----------------|
| PostgreSQL | 5433      | 5432           |
| YACI Store | 8081      | 8080           |
| API        | 8080      | 8080           |

PostgreSQL uses **5433** on the host to avoid conflicts with local PostgreSQL installations.

Database connection string: `postgresql://postgres:postgres@localhost:5433/administration_data`

## Code Conventions

- Rust 2021 edition, **Axum 0.7** web framework
- **SQLx** for database queries (compile-time checked)
- **utoipa** OpenAPI decorators on all endpoints and models
- Consistent API response envelope: `{ data, pagination?, meta.timestamp }`
- Follow existing patterns in the codebase
- Add tests for new code

## Database

- Schema source of truth: `database/schema/treasury.sql`
- Init scripts: `database/init/` — run on first Docker PostgreSQL start
- For schema changes: create incremental migration files, don't edit `treasury.sql` directly for running systems
- YACI Store schema is auto-managed by Flyway — **never modify manually**

## Indexer

YACI Store is a **black-box dependency**. Only modify configuration and plugins:

- `indexer/application.properties` — indexer config
- `indexer/config/application-plugins.yml` — plugin configuration
- `indexer/plugins/scripts/treasury-filter.mvel` — MVEL filter script

Never modify `yaci-store.jar` or YACI Store internals. Primary network: Mainnet (`backbone.cardano.iog.io:3001`).

## CI/CD

- **`ci.yml`**: runs `cargo build --release && cargo test` on push/PR to main/develop
- **`push-to-ecr.yaml`**: builds Docker image and pushes to AWS ECR on push to main (or manual dispatch)
- Deployment: Helm chart bump in a separate repo

## Gotchas

- **Startup ordering**: YACI Store must be running and synced before the API sync service can process events. The sync service (`api/src/services/sync.rs`) waits for YACI Store tables to exist.
- **Port 5433**: PostgreSQL is on host port 5433, not 5432.
- **`.env` not committed**: copy `.env.example` and configure before first run.
- **UTXO pruning**: YACI Store prunes spent UTXOs — historical UTXO data may not be available.
- **Cold replay vs continuous operation**: The milestone-event chain trace (`find_project_from_inputs`) needs UTXO history to link withdraw/complete/pause/resume to a project. The Postgres triggers installed by `install_utxo_history_triggers` (in `api/src/services/sync.rs`) capture every script-address UTXO into `treasury.utxo_history` synchronously with YACI Store's INSERT, so pruning no longer drops chain-trace inputs. Triggers only protect from the moment they're armed — to recover pre-existing pruned data, wipe the database volume and re-sync with the API running so the triggers arm before YACI Store ingests. See [`docs/known-issues.md`](docs/known-issues.md) `KI-CR-01` and `KI-UTX-01`.
- **Large JAR**: `indexer/yaci-store.jar` is ~108MB and committed to the repo. Don't regenerate unnecessarily.
- **Inline datums**: `store.script.enabled=true` in YACI Store config enables milestone datum data (amounts, time limits, pause flags). Requires full re-sync after enabling.
- **Milestone archiving**: Filter `WHERE NOT archived` for current milestones. Archived rows are historical versions.

## Key File Locations

| Purpose            | Path                                  |
|--------------------|---------------------------------------|
| API entry point    | `api/src/main.rs`                     |
| API routes         | `api/src/routes/v1/`                  |
| API models         | `api/src/models/v1.rs`                |
| Event processing   | `api/src/services/event_processor.rs` |
| Sync service       | `api/src/services/sync.rs`            |
| DB schema          | `database/schema/treasury.sql`        |
| DB init scripts    | `database/init/`                      |
| Docker setup       | `docker-compose.yml`                  |
| Dev script         | `dev.sh`                              |
| Indexer config     | `indexer/application.properties`      |
| Plugin config      | `indexer/config/application-plugins.yml` |
| Treasury filter    | `indexer/plugins/scripts/treasury-filter.mvel` |
| CI                 | `.github/workflows/ci.yml`            |
| ECR push           | `.github/workflows/push-to-ecr.yaml`  |
