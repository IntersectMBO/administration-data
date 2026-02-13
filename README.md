# Intersect Budget Administration Data

A system to collect on-chain Budget administration data for instances of [treasury-contracts](https://github.com/SundaeSwap-finance/treasury-contracts)
and offer a simple API.

Using YACI Store for blockchain indexing,
PostgreSQL for storage
and a Rust-based API backend.

Swagger Docs hosted at
- [administration.info.intersectmbo.org/docs](https://administration.info.intersectmbo.org/docs)

API instance hosted at
- [administration.info.intersectmbo.org/api/v1](https://administration.info.intersectmbo.org/api/v1)

## Architecture

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Cardano Node   │────▶│   YACI Store    │────▶│   PostgreSQL    │
│   (Mainnet)     │     │   (Indexer)     │     │   (Database)    │
└─────────────────┘     └─────────────────┘     └────────┬────────┘
                                                         │
                                                         ▼
                                                ┌─────────────────┐
                                                │   Rust API      │
                                                │   (Backend)     │
                                                └─────────────────┘
```

## Project Structure

```
administration-data/
├── .env                        # Environment variables (not committed)
├── .env.example                # Environment variable template
├── indexer/                    # YACI Store indexer configuration
│   ├── application.properties
│   ├── config/
│   │   └── application-plugins.yml  # Plugin filter configuration
│   ├── plugins/
│   │   └── scripts/
│   │       └── treasury-filter.mvel # Metadata filter logic
│   └── README.md
├── api/                        # Rust API backend
│   ├── src/
│   │   ├── main.rs
│   │   ├── routes/v1/          # V1 API endpoints
│   │   ├── models/v1.rs        # API models with OpenAPI
│   │   ├── openapi.rs          # Swagger/OpenAPI config
│   │   └── services/           # Background sync & event processing
│   ├── Cargo.toml
│   └── README.md               # Full API documentation
├── database/
│   ├── init/                   # Docker PostgreSQL init scripts (run on first start)
│   │   └── 02-treasury-schema.sql
│   └── schema/
│       └── treasury.sql        # Treasury schema (single source of truth)
├── docs/                       # Documentation
│   └── architecture.md         # Data flow diagrams
├── .github/                    # CI/CD workflows
├── docker-compose.yml
└── dev.sh                      # Development helper script
```

## Quick Start

### Prerequisites

- Docker and Docker Compose
- Create a `.env` based on [`.env.example`](./.env.example)

### Start All Services

```bash
./dev.sh start
```

This starts:
- **PostgreSQL** on port 5433 (host) / 5432 (container)
- **YACI Store Indexer** on port 8081 (syncs Cardano blockchain)
- **Administration API** on port 8080

### Verify Services

```bash
# Check service status
./dev.sh status

# Test API health
curl http://localhost:8080/health
# Returns: OK

# Get API status
curl http://localhost:8080/api/v1/status

# View interactive API docs
open http://localhost:8080/docs

# Check indexer sync status
curl http://localhost:8081/api/v1/blocks/latest
```

## API Endpoints

Base URL: `http://localhost:8080`

Interactive documentation available at `/docs` (Swagger UI).

### Core Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check |
| `GET /docs` | Swagger UI (interactive API docs) |
| `GET /api/v1/status` | API status and sync info |
| `GET /api/v1/statistics` | Comprehensive statistics |

### Treasury

| Endpoint | Description |
|----------|-------------|
| `GET /api/v1/treasury` | Treasury contract details with statistics |
| `GET /api/v1/treasury/utxos` | Treasury UTXOs |
| `GET /api/v1/treasury/events` | Treasury-level events |

### Vendor Contracts (Projects)

| Endpoint | Description |
|----------|-------------|
| `GET /api/v1/vendor-contracts` | List all vendor contracts (with pagination, filtering, search) |
| `GET /api/v1/vendor-contracts/:project_id` | Get vendor contract details |
| `GET /api/v1/vendor-contracts/:project_id/milestones` | Get project milestones |
| `GET /api/v1/vendor-contracts/:project_id/events` | Get project event history |
| `GET /api/v1/vendor-contracts/:project_id/utxos` | Get project UTXOs |

### Milestones

| Endpoint | Description |
|----------|-------------|
| `GET /api/v1/milestones` | List all milestones (with pagination, filtering) |
| `GET /api/v1/milestones/:id` | Get milestone details |

### Events

| Endpoint | Description |
|----------|-------------|
| `GET /api/v1/events` | List all events (with pagination, filtering) |
| `GET /api/v1/events/recent` | Recent activity feed |
| `GET /api/v1/events/:tx_hash` | Get event by transaction hash |

**[Full API Documentation →](api/README.md)**

## YACI Store Indexer API

The YACI Store indexer exposes its own API on port 8081:

| Endpoint | Description |
|----------|-------------|
| `GET /api/v1/blocks/latest` | Latest synced block |
| `GET /api/v1/blocks/:number` | Block by number |
| `GET /api/v1/txs/:hash` | Transaction by hash |
| `GET /api/v1/addresses/:addr/utxos` | UTXOs by address |
| `GET /actuator/health` | Indexer health status |

## Configuration

### Treasury Reserve Contract

We can configure the Treasury Reserve instance that we index for via the `.env` file at the project root:

```bash
# .env
TREASURY_INSTANCE=9e65e4ed7d6fd86fc4827d2b45da6d2c601fb920e8bfd794b8ecc619
```

This environment variable is passed to the indexer container via `docker-compose.yml` and used by the `treasury-filter.mvel` plugin script to filter metadata.

### Sync Configuration

Limitation: this is only configured for Mainnet currently

The sync start point is configured via environment variables in `.env`:

```bash
STORE_CARDANO_SYNC_START_SLOT=160964954
STORE_CARDANO_SYNC_START_BLOCKHASH=560c7537831007f9670d287b15a69ba18a322b1edc39c0c23ccab3c12ad77b9f
```

Network settings (host, port, protocol magic) are in `indexer/application.properties`.

## Development Commands

```bash
./dev.sh start      # Start all services
./dev.sh stop       # Stop all services
./dev.sh status     # Check service status
./dev.sh logs       # Show all logs
./dev.sh logs api   # Show API logs only
./dev.sh logs indexer  # Show indexer logs only
./dev.sh build      # Rebuild Docker images
./dev.sh clean      # Remove containers and volumes
./dev.sh help       # Show help
```

## Database

### Schema

The system uses two schemas:

**yaci_store** - Raw blockchain data, managed automatically by YACI Store via Flyway migrations:

| Table | Description | Filtering |
|-------|-------------|-----------|
| `yaci_store.block` | Blockchain blocks | All blocks stored |
| `yaci_store.address_utxo` | Treasury UTXOs | Only treasury stake credential |
| `yaci_store.transaction_metadata` | TOM metadata | Only label 1694 |

**treasury** - Normalized application data (defined in [`database/schema/treasury.sql`](database/schema/treasury.sql)):

| Table | Description |
|-------|-------------|
| `treasury.treasury_contracts` | Treasury reserve contracts (TRSC) |
| `treasury.vendor_contracts` | Vendor/project contracts (PSSC) |
| `treasury.milestones` | Project milestones |
| `treasury.events` | All TOM event audit log |
| `treasury.utxos` | UTXO tracking for event linking |

### Connecting to Database

```bash
# Via docker
docker exec -it administration-postgres psql -U postgres -d administration_data

# Via local psql (port 5433)
psql -h localhost -p 5433 -U postgres -d administration_data
```

### Key Queries

```sql
-- Latest synced block
SELECT * FROM yaci_store.block ORDER BY number DESC LIMIT 5;

-- Treasury summary
SELECT * FROM treasury.v_treasury_summary;

-- Vendor contracts with financials
SELECT project_id, project_name, status,
       initial_amount_lovelace / 1000000 as allocated_ada,
       total_disbursed_lovelace / 1000000 as disbursed_ada
FROM treasury.v_vendor_contracts_summary;

-- Recent events
SELECT * FROM treasury.v_events_with_context
ORDER BY block_time DESC LIMIT 10;
```

### Plugin Filtering

YACI Store plugins filter blockchain data to only store treasury-relevant information:

- **Metadata Filter**: Only metadata with label `1694` (TOM standard) AND the treasury instance configured via `TREASURY_INSTANCE` in `.env`

This reduces database size by ~95% while keeping all treasury data.

## Component Documentation

- [Architecture & Data Flow](docs/architecture.md) - System architecture and data flow diagrams
- [API Documentation](api/README.md) - Full API reference
- [Indexer Setup](indexer/README.md) - YACI Store configuration
- [Database Schema](database/schema/) - Treasury schema definitions

## License

See [LICENSE](./LICENSE).
