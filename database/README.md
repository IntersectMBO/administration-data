# Database Schema

This directory contains database schema definitions for the administration data system.

## Schema Overview

The system uses two schemas:
1. **YACI Store schema** (`yaci_store`) - Created automatically by YACI Store for blockchain data
2. **Treasury schema** (`treasury`) - Normalized application data for treasury tracking

## Treasury Schema Tables

### treasury.treasury_contracts
Stores treasury reserve contract instances (TRSC). Singleton in our deployment.

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| contract_instance | TEXT | On-chain instance identifier (policy ID, unique) |
| contract_address | TEXT | Script address |
| stake_credential | TEXT | Shared stake credential |
| name | TEXT | Human-readable name |
| publish_tx_hash | VARCHAR(64) | Publish transaction |
| publish_time | BIGINT | Publish block time |
| initialized_tx_hash | VARCHAR(64) | Initialize transaction |
| initialized_at | BIGINT | Initialize block time |
| permissions | JSONB | Permission rules |
| status | TEXT | active/paused |

### treasury.vendor_contracts
Singleton row for the shared on-chain vendor contract (PSSC) script address — the *one* address every project's UTXOs sit at, distinguished only by inline datum.

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| treasury_id | INT | FK to treasury_contracts |
| address | TEXT | Shared PSSC script address (unique) |
| stake_credential | TEXT | Stake credential portion of the address |

### treasury.projects
One row per `fund` event (e.g. `EC-0008-25`). 42 rows in our deployment. Identified by `project_id`; funds and milestones live at the shared PSSC above, distinguished by inline datum.

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| treasury_id | INT | FK to treasury_contracts |
| project_id | TEXT | Logical identifier (e.g., "EC-0008-25", unique) |
| other_identifiers | TEXT[] | Related IDs from `otherIdentifiers` array |
| project_name | TEXT | Label from fund event |
| description | TEXT | Project description |
| vendor_address | TEXT | Payment destination (`vendor.label` in metadata) |
| contract_address | TEXT | PSSC script address (from fund tx output) |
| vendor_payment_key_hash | TEXT | Comma-joined hex hashes (multi-party datums produce multiple) |
| fund_tx_hash | VARCHAR(64) | Fund transaction |
| fund_slot | BIGINT | Blockchain slot |
| fund_block_time | BIGINT | Block timestamp |
| initial_amount_lovelace | BIGINT | Initial funding amount (from tx output) |
| status | TEXT | active/paused/completed/cancelled |
| datum_parse_error | TEXT | Set when fund datum parse failed; cleared on success |

### treasury.milestones
Stores milestone data for each project. Uses 4 independent boolean flags instead of a linear status; archive model preserves prior versions via `superseded_by`.

State flags (all default FALSE, all independent):
- `evidence_provided` — vendor submitted a `complete` event
- `withdrawn` — vendor pulled funds via a `withdraw` event
- `paused` — derived from inline-datum parsing in `update_milestone_pause_from_datum` (`api/src/services/event_processor.rs`); not present in metadata
- `archived` — milestone replaced by a `modify` event; the new row is linked via `superseded_by` and queries for current state should include `WHERE NOT archived`

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| project_db_id | INT | FK to projects (CASCADE) |
| milestone_id | TEXT | Logical identifier (e.g., "m-0") |
| milestone_order | INT | Position (1, 2, 3...) |
| label | TEXT | Milestone name |
| description | TEXT | Detailed description |
| acceptance_criteria | TEXT | Completion criteria |
| amount_lovelace | BIGINT | Lovelace amount from datum |
| time_limit | BIGINT | POSIXTime in milliseconds from datum |
| withdrawn | BOOLEAN | Vendor withdrew payment |
| evidence_provided | BOOLEAN | Vendor submitted completion evidence |
| paused | BOOLEAN | Oversight committee paused this milestone (datum-derived) |
| archived | BOOLEAN | Milestone replaced by modify event |
| withdraw_tx_hash | VARCHAR(64) | Withdrawal transaction |
| withdraw_time | BIGINT | Withdrawal timestamp |
| withdraw_amount | BIGINT | Withdrawn amount |
| complete_tx_hash | VARCHAR(64) | Completion transaction |
| complete_time | BIGINT | Completion timestamp |
| complete_description | TEXT | Completion notes |
| evidence | JSONB | Evidence array |
| archived_by_tx_hash | VARCHAR(64) | Modify tx that archived this milestone |
| archived_at | BIGINT | Archive timestamp |
| superseded_by | INT | FK to replacement milestone |
| datum_parse_error | TEXT | Set when datum parse failed for this milestone |

A partial unique index `idx_milestone_active_unique` on `(project_db_id, milestone_id) WHERE NOT archived` ensures only one active row per logical milestone.

### treasury.events
Audit log of all TOM (Treasury Oversight Metadata) events.

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| tx_hash | VARCHAR(64) | Transaction hash (unique) |
| slot | BIGINT | Blockchain slot |
| block_number | BIGINT | Block number |
| block_time | BIGINT | Block timestamp |
| event_type | TEXT | Event type |
| treasury_id | INT | FK to treasury_contracts |
| project_db_id | INT | FK to projects |
| milestone_id | INT | FK to milestones |
| amount_lovelace | BIGINT | Amount involved |
| reason | TEXT | Justification (pause/cancel/modify) |
| destination | JSONB | Destination object `{label, details}` from disburse events |
| metadata | JSONB | Original TOM metadata body |
| created_at | TIMESTAMPTZ | Row insert timestamp |

### treasury.utxo_history
Persistent UTXO history at treasury-related script addresses. Two responsibilities:
1. **Chain trace seed** — outputs of `fund` txs are written here with `project_db_id` set, so `find_project_from_inputs` can later trace milestone-event inputs back to a project.
2. **Datum cache** — `inline_datum_cbor` is stored on each UTXO so pause/resume datum parsing (`update_milestone_pause_from_datum`) keeps working after YACI Store has pruned the row out of `yaci_store.address_utxo`.

Population: Postgres triggers installed by `install_utxo_history_triggers` (`api/src/services/sync.rs`) capture every script-address (`addr1x*`) UTXO from `yaci_store.address_utxo` synchronously on INSERT, and flag rows as spent on `yaci_store.tx_input` INSERT. `pre_fetch_utxos` is a defensive backstop run during event processing.

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| tx_hash | VARCHAR(64) | Transaction hash |
| output_index | SMALLINT | Output index |
| address | TEXT | Owner address |
| address_type | TEXT | treasury/vendor_contract/vendor |
| project_db_id | INT | FK to projects (chain-trace seed; NULL on non-script outputs) |
| lovelace_amount | BIGINT | Amount |
| inline_datum_cbor | TEXT | Hex-encoded inline datum (cached for post-prune datum parsing) |
| slot | BIGINT | Creation slot |
| block_number | BIGINT | Block number |
| spent | BOOLEAN | Is spent? |
| spent_tx_hash | VARCHAR(64) | Spending transaction |
| spent_slot | BIGINT | When spent |

`UNIQUE(tx_hash, output_index)`.

### treasury.sync_status
Tracks synchronization progress. Two rows by convention:
- `sync_type='events'` — heartbeat for the TOM-event sync loop. `updated_at` bumps on every poll, including idle ticks.
- `sync_type='utxos'` — checkpoint for the UTXO pre-fetch worker.

`last_slot` advances only on contiguous success — if an event fails mid-batch the watermark stays put so the failed event is retried on the next poll. A separate task runs `sync_all_events` every 10 minutes as an idempotent backfill safety net (see [`KI-SY-02`](../docs/known-issues.md)).

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| sync_type | TEXT | events/utxos (unique) |
| last_slot | BIGINT | Last processed slot |
| last_block | BIGINT | Last processed block |
| last_tx_hash | VARCHAR(64) | Last processed tx |
| updated_at | TIMESTAMPTZ | Last update time |

## Database Views

### treasury.v_treasury_summary
Treasury contracts with aggregated statistics and financials.

```sql
SELECT * FROM treasury.v_treasury_summary;
```

Fields: `treasury_id`, `contract_instance`, `contract_address`, `stake_credential`, `status`, `publish_tx_hash`, `publish_time`, `initialized_tx_hash`, `initialized_at`, `permissions`, `project_count`, `active_contracts`, `completed_contracts`, `cancelled_contracts`, `treasury_balance`, `utxo_count`, `total_events`, `last_event_time`, `created_at`, `updated_at`.

`treasury_balance` and `utxo_count` are sourced from `treasury.utxo_history` (unspent UTXOs at the treasury script address).

### treasury.v_projects_summary
Projects with milestone counts, financials, and UTXO balance.

```sql
SELECT * FROM treasury.v_projects_summary;
```

Fields: `id`, `treasury_id`, `project_id`, `other_identifiers`, `project_name`, `description`, `vendor_address`, `contract_address`, `fund_tx_hash`, `fund_slot`, `fund_block_time`, `initial_amount_lovelace`, `status`, `created_at`, `updated_at`, `treasury_instance`, `total_milestones`, `pending_milestones`, `completed_milestones`, `withdrawn_milestones`, `paused_milestones`, `total_withdrawn_lovelace`, `current_balance_lovelace`, `utxo_count`, `last_event_time`, `event_count`.

### treasury.v_events_with_context
Events with full treasury/project/milestone context.

```sql
SELECT * FROM treasury.v_events_with_context ORDER BY block_time DESC;
```

Fields: `id`, `tx_hash`, `slot`, `block_number`, `block_time`, `event_type`, `amount_lovelace`, `reason`, `destination`, `metadata`, `created_at`, `treasury_instance`, `project_id`, `project_name`, `project_address`, `milestone_id`, `milestone_label`, `milestone_order`.

### treasury.v_recent_events
Same projection as `v_events_with_context`, ordered by `slot DESC` for activity feeds.

### treasury.v_financial_summary
Financial summary showing allocated vs withdrawn vs remaining.

```sql
SELECT * FROM treasury.v_financial_summary;
```

Fields: `treasury_id`, `contract_instance`, `total_allocated_lovelace`, `total_withdrawn_lovelace`, `total_remaining_lovelace`, `treasury_balance_lovelace`, `project_balance_lovelace`, `project_count`, `active_project_count`.

### treasury.v_milestone_timeline
Milestones with project context.

```sql
SELECT * FROM treasury.v_milestone_timeline;
```

Fields: `id`, `milestone_id`, `milestone_order`, `label`, `description`, `acceptance_criteria`, `amount_lovelace`, `time_limit`, `withdrawn`, `evidence_provided`, `archived`, `complete_tx_hash`, `complete_time`, `complete_description`, `evidence`, `withdraw_tx_hash`, `withdraw_time`, `withdraw_amount`, `archived_by_tx_hash`, `archived_at`, `superseded_by`, `project_id`, `project_name`, `vendor_address`.

## Running Migrations

The treasury schema is created on first PostgreSQL container start by `database/init/02-treasury-schema.sql`. The API also installs the `treasury.utxo_history` triggers at startup via `install_utxo_history_triggers` (`api/src/services/sync.rs`); these arm before YACI Store ingests so a fresh sync captures every script-address UTXO before pruning runs.

### Using psql directly

```bash
# Connect to database
docker exec -it administration-postgres psql -U postgres -d administration_data

# Run schema file
\i /path/to/database/schema/treasury.sql
```

Or:

```bash
docker exec -T administration-postgres psql -U postgres -d administration_data < database/schema/treasury.sql
```

## YACI Store Tables

YACI Store creates its own tables in the `yaci_store` schema. Key tables include:
- `block` - Block information
- `transaction` - Transaction data
- `address_utxo` - UTXO data by address
- `transaction_metadata` - Transaction metadata by label
- `tx_input` - Transaction inputs
- `cursor_` - Current sync position

These tables are automatically created and maintained by YACI Store via Flyway.

## Indexes

The schema includes indexes for:
- Primary key lookups
- Foreign key relationships
- Status filtering
- Time-based ordering (`fund_block_time`, `block_time`)
- Text search (`project_id`, `project_name`, `description`)
- UTXO queries (unspent UTXOs, address lookups)
- A partial unique index on milestones to enforce one active row per `(project_db_id, milestone_id)`

## Example Queries

```sql
-- Get all active projects with their financials
SELECT
    project_id,
    project_name,
    initial_amount_lovelace / 1000000.0 as allocated_ada,
    total_withdrawn_lovelace / 1000000.0 as withdrawn_ada,
    current_balance_lovelace / 1000000.0 as balance_ada,
    total_milestones,
    withdrawn_milestones
FROM treasury.v_projects_summary
WHERE status = 'active'
ORDER BY fund_block_time DESC;

-- Get recent events with context
SELECT
    event_type,
    project_id,
    project_name,
    milestone_label,
    amount_lovelace / 1000000.0 as amount_ada,
    TO_TIMESTAMP(block_time) as event_time
FROM treasury.v_events_with_context
ORDER BY block_time DESC
LIMIT 20;

-- Financial summary
SELECT
    contract_instance,
    total_allocated_lovelace / 1000000.0 as total_allocated_ada,
    total_withdrawn_lovelace / 1000000.0 as total_withdrawn_ada,
    total_remaining_lovelace / 1000000.0 as remaining_ada,
    project_count,
    active_project_count
FROM treasury.v_financial_summary;
```
