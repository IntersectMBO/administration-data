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
- **Treasury Contract (TRSC)** → contains multiple **Vendor Contracts (PSSC)** → each has **Milestones**

### Event Types
publish, initialize, fund, complete, disburse, withdraw, pause, resume, modify, cancel, sweep, reorganize

### Financial Model
- All amounts are in **lovelace** (1 ADA = 1,000,000 lovelace)
- Milestone lifecycle: pending → completed → disbursed

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
- **Large JAR**: `indexer/yaci-store.jar` is ~108MB and committed to the repo. Don't regenerate unnecessarily.

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
