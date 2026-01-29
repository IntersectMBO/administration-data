# Database Schema

This directory contains database schema definitions for the administration data system.

## Schema Overview

The system uses two schemas:
1. **YACI Store schema** (`yaci_store`) - Created automatically by YACI Store for blockchain data
2. **Treasury schema** (`treasury`) - Normalized application data for treasury tracking

## Treasury Schema Tables

### treasury.treasury_contracts
Stores treasury reserve contract instances (TRSC).

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| contract_instance | TEXT | On-chain instance identifier (policy ID) |
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
Stores vendor/project contract instances (PSSC).

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| treasury_id | INT | FK to treasury_contracts |
| project_id | TEXT | Logical identifier (e.g., "EC-0008-25") |
| other_identifiers | TEXT[] | Related IDs |
| project_name | TEXT | Project label |
| description | TEXT | Project description |
| vendor_name | TEXT | Vendor name |
| vendor_address | TEXT | Payment destination |
| contract_url | TEXT | Link to agreement |
| contract_address | TEXT | PSSC script address |
| fund_tx_hash | VARCHAR(64) | Fund transaction |
| fund_slot | BIGINT | Fund slot |
| fund_block_time | BIGINT | Fund block time |
| initial_amount_lovelace | BIGINT | Initial funding amount |
| status | TEXT | active/paused/completed/cancelled |

### treasury.milestones
Stores milestone data for each vendor contract.

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| vendor_contract_id | INT | FK to vendor_contracts |
| milestone_id | TEXT | Logical identifier (e.g., "m-0") |
| milestone_order | INT | Position (1, 2, 3...) |
| label | TEXT | Milestone name |
| description | TEXT | Detailed description |
| acceptance_criteria | TEXT | Completion criteria |
| amount_lovelace | BIGINT | Allocated amount |
| status | TEXT | pending/completed/disbursed |
| complete_tx_hash | VARCHAR(64) | Completion transaction |
| complete_time | BIGINT | Completion timestamp |
| complete_description | TEXT | Completion notes |
| evidence | JSONB | Evidence array |
| disburse_tx_hash | VARCHAR(64) | Disbursement transaction |
| disburse_time | BIGINT | Disbursement timestamp |
| disburse_amount | BIGINT | Disbursed amount |

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
| vendor_contract_id | INT | FK to vendor_contracts |
| milestone_id | INT | FK to milestones |
| amount_lovelace | BIGINT | Amount involved |
| reason | TEXT | Justification (pause/cancel/modify) |
| destination | TEXT | Destination label (disburse) |
| metadata | JSONB | Original TOM metadata body |

### treasury.utxos
Tracks UTXOs at treasury-related addresses for event linking.

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| tx_hash | VARCHAR(64) | Transaction hash |
| output_index | SMALLINT | Output index |
| address | TEXT | Owner address |
| address_type | TEXT | treasury/vendor_contract/vendor |
| vendor_contract_id | INT | FK to vendor_contracts |
| lovelace_amount | BIGINT | Amount |
| slot | BIGINT | Creation slot |
| block_number | BIGINT | Block number |
| spent | BOOLEAN | Is spent? |
| spent_tx_hash | VARCHAR(64) | Spending transaction |
| spent_slot | BIGINT | When spent |

### treasury.sync_status
Tracks synchronization progress.

| Column | Type | Description |
|--------|------|-------------|
| id | SERIAL | Primary key |
| sync_type | TEXT | events/utxos |
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

Fields: treasury_id, contract_instance, contract_address, stake_credential, name, status, publish_tx_hash, publish_time, initialized_tx_hash, initialized_at, permissions, vendor_contract_count, active_contracts, completed_contracts, cancelled_contracts, treasury_balance, utxo_count, total_events, last_event_time, created_at, updated_at

### treasury.v_vendor_contracts_summary
Vendor contracts with milestone counts, financials, and UTXO balance.

```sql
SELECT * FROM treasury.v_vendor_contracts_summary;
```

Fields: id, treasury_id, project_id, other_identifiers, project_name, description, vendor_name, vendor_address, contract_url, contract_address, fund_tx_hash, fund_slot, fund_block_time, initial_amount_lovelace, status, created_at, updated_at, treasury_instance, treasury_name, total_milestones, pending_milestones, completed_milestones, disbursed_milestones, total_disbursed_lovelace, current_balance_lovelace, utxo_count, last_event_time, event_count

### treasury.v_events_with_context
Events with full treasury/project/milestone context.

```sql
SELECT * FROM treasury.v_events_with_context ORDER BY block_time DESC;
```

Fields: id, tx_hash, slot, block_number, block_time, event_type, amount_lovelace, reason, destination, metadata, created_at, treasury_instance, treasury_name, project_id, project_name, vendor_name, project_address, milestone_id, milestone_label, milestone_order

### treasury.v_financial_summary
Financial summary showing allocated vs disbursed vs remaining.

```sql
SELECT * FROM treasury.v_financial_summary;
```

Fields: treasury_id, contract_instance, treasury_name, total_allocated_lovelace, total_disbursed_lovelace, total_remaining_lovelace, treasury_balance_lovelace, project_balance_lovelace, project_count, active_project_count

### treasury.v_milestone_timeline
Milestones with vendor contract context.

```sql
SELECT * FROM treasury.v_milestone_timeline;
```

Fields: id, milestone_id, milestone_order, label, description, acceptance_criteria, amount_lovelace, status, complete_tx_hash, complete_time, complete_description, evidence, disburse_tx_hash, disburse_time, disburse_amount, project_id, project_name, vendor_address

### treasury.v_recent_events
Events with context, ordered by slot descending (for recent activity).

```sql
SELECT * FROM treasury.v_recent_events LIMIT 10;
```

## Running Migrations

### Using the API (automatic)

The API automatically creates/updates the schema on startup via `db::init_treasury_schema()`.

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

These tables are automatically created and maintained by YACI Store.

## Indexes

The schema includes indexes for:
- Primary key lookups
- Foreign key relationships
- Status filtering
- Time-based ordering (fund_block_time, block_time)
- Text search (project_id, project_name, vendor_name)
- UTXO queries (unspent UTXOs, address lookups)

## Example Queries

```sql
-- Get all active projects with their financials
SELECT
    project_id,
    project_name,
    vendor_name,
    initial_amount_lovelace / 1000000.0 as allocated_ada,
    total_disbursed_lovelace / 1000000.0 as disbursed_ada,
    current_balance_lovelace / 1000000.0 as balance_ada,
    total_milestones,
    disbursed_milestones
FROM treasury.v_vendor_contracts_summary
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
    total_disbursed_lovelace / 1000000.0 as total_disbursed_ada,
    total_remaining_lovelace / 1000000.0 as remaining_ada,
    project_count,
    active_project_count
FROM treasury.v_financial_summary;
```
