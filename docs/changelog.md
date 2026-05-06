# API Changelog

This file tracks user-visible changes to the `/api/v1/` surface and the
treasury data pipeline. Each release ships as a single commit on `main` (or
the equivalent merge). Pre-1.0 versions allowed breaking changes; the
project is now operating under a 1.x line and breaking changes here are
flagged as such.

## v2.1.0 — 2026-05-05

Adds a vendor-contract-wide UTxO view and inlines per-project UTxO refs on
the project detail response. Both changes are additive — no breaking
changes to existing endpoints or shapes.

### Added

- **`GET /api/v1/vendor-contract/utxos`** — paginated list of every
  currently-unspent UTxO at the shared PSSC, each row labeled with its
  owning project (`project_id`, `project_name`, `project_status`,
  `project_db_id`). Lets clients enumerate live vendor-contract state in a
  single call instead of fanning out across every project. Same
  unspent-source-of-truth pattern as `/treasury/utxos` and
  `/projects/:id/utxos` (`yaci_store.address_utxo` ⨯ anti-join on
  `yaci_store.tx_input`).
- **`ProjectDetail.current_utxos`** — `GET /api/v1/projects/{project_id}`
  now includes a `current_utxos` array of `{ tx_hash, output_index,
  lovelace_amount, slot }` so a single call gives the project's full live
  state. Sum of `lovelace_amount` equals the existing
  `financials.current_balance_lovelace`. `ProjectSummary` (the list
  endpoint item shape) is unchanged.

### Schema

- No DB migration. Both features are read-only joins over existing
  columns: `treasury.utxo_history.project_db_id` (already populated by
  fund events + chain tracing) joined to `treasury.projects` and
  `yaci_store.address_utxo`.

## v2.0.0 — 2026-05-01

Semantic rename pass: split "vendor contract" into the *singleton on-chain
script address* (the shared PSSC, one row) and the 42 *projects* (one per
fund event) that sit at it. Old paths are gone (404), not aliased — pre-1.0
contract still applies.

### Breaking — paths

- `/api/v1/vendor-contracts` → `/api/v1/projects` (list + filter)
- `/api/v1/vendor-contracts/{project_id}` → `/api/v1/projects/{project_id}`
- `/api/v1/vendor-contracts/{project_id}/milestones` →
  `/api/v1/projects/{project_id}/milestones`
- `/api/v1/vendor-contracts/{project_id}/events` →
  `/api/v1/projects/{project_id}/events`
- `/api/v1/vendor-contracts/{project_id}/utxos` →
  `/api/v1/projects/{project_id}/utxos`

### Added

- **`GET /api/v1/vendor-contract`** — singleton: returns
  `{ address, stake_credential, projects: { total, by_status: {...} } }`.
  The shared PSSC every project sits at.
- **`GET /api/v1/milestones/{project_id}`** — paginated milestones list
  under the `/milestones/` root. Equivalent to
  `/projects/{project_id}/milestones`; differs only in URL hierarchy.
- **`GET /api/v1/milestones/by-id/{id}`** — single milestone by integer
  database ID. The previous `/milestones/{id}` lookup moved here to free
  the parameterised `/milestones/{project_id}` slot for project lookups.

### Breaking — response shapes

- `StatusResponse.totals.vendor_contracts` → `totals.projects`.
- `TreasuryStatistics.vendor_contract_count` → `project_count`.
- `Milestone.vendor_contract_id` (FK) is no longer exposed; the canonical
  link is via `project_id` (text).
- Numerous internal struct/field renames are not visible in the JSON wire
  format but appear in the OpenAPI schema list.

### Schema

- Renamed `treasury.vendor_contracts` → `treasury.projects`.
- Renamed FK column `vendor_contract_id` → `project_db_id` in
  `treasury.events`, `treasury.milestones`, `treasury.utxo_history`.
- New singleton `treasury.vendor_contracts (id, treasury_id, address,
  stake_credential, …)` stores one row per shared PSSC.
- Renamed view `v_vendor_contracts_summary` → `v_projects_summary`;
  view bodies updated to use new names.
- Trigger `trg_vendor_contracts_updated_at` → `trg_projects_updated_at`.

### Internal renames (impact code readers, not the API surface)

- `find_vendor_contract_from_inputs` → `find_project_from_inputs`.
- `parse_vendor_contract_datum` → `parse_project_datum`;
  `ParsedVendorDatum` → `ParsedProjectDatum`.

### Migration

Existing deployments must wipe `treasury` schema and re-sync (the
`utxo_history` Postgres triggers come back via the API's startup hook).
There is no in-place column-rename migration shipped — pre-1.x stance.

## v1.1.0 — 2026-05-01

API consistency pass. Several breaking response-shape changes — frontends
update once and stay on `/api/v1/`.

### Breaking

- **`/api/v1/status` restructured.** Old flat fields
  (`last_sync_slot`, `last_sync_block`, `last_sync_time`,
  `database_connected`, `total_events`, `total_vendor_contracts`)
  replaced with nested groups:
  - `database: { connected, checked_at }` — server-side ISO.
  - `sync: { heartbeat, last_event_processed }` — heartbeat is the
    server-side ISO of the last sync poll; `last_event_processed` is the
    on-chain block time of the most recent TOM event the API has written
    (`ChainTime`).
  - `chain: { indexer_block, indexer_slot, indexer_time }` — what YACI
    Store has reached. `indexer_time` is `ChainTime`.
  - `totals: { events, vendor_contracts, events_by_type }`.

### Other breaking

- **Timestamps**: every on-chain block-time field is now an object
  `{ "unix": 1777623100, "iso": "2026-05-01T08:11:40Z" }` instead of a
  bare integer. Affects `EventResponse.block_time`,
  `VendorContract*.fund_time` and `last_event_time`,
  `MilestoneCompletion.time`, `MilestoneWithdrawal.time`,
  `MilestoneArchiveInfo.archived_at`, and `TreasuryResponse.publish_time`
  / `initialized_at`. Server-side timestamps (`created_at`,
  `updated_at`, `last_updated`) remain ISO strings.
- **Errors**: every non-2xx response now returns a JSON body
  `{ "error": { "code", "message", "details"? }, "meta": { "timestamp" } }`
  instead of an empty body. `code` values: `not_found`, `bad_request`,
  `internal`.
- **Pagination**: `/api/v1/treasury/utxos`,
  `/api/v1/vendor-contracts/:project_id/milestones`, and
  `/api/v1/vendor-contracts/:project_id/utxos` now return
  `{ data, pagination, meta }` with `?page=1&limit=50` (max 100).
  Previous responses returned an unbounded array under `data`.
- **`destination` on disburse events**: now JSONB preserving the full TOM
  `{ label, details }` object instead of flattened to a string.
  Released earlier in v1.0.x but listed here for completeness.
- **`vendor_name` and `contract_url`**: dropped from
  `treasury.vendor_contracts` and from API responses. They were always
  NULL; not part of the TOM spec.

### Added

- **`?q=`** full-text search on `/api/v1/events` matching against
  `reason`, `destination::text`, and `metadata::text` (case-insensitive
  substring).
- **`?from_time=` / `?to_time=`** filters on `/api/v1/milestones` matching
  whichever of `complete_time` or `withdraw_time` is set on the row.
- **OpenAPI**: per-`event_type` field-applicability descriptions on
  `EventResponse` (which fields apply to which event type), and
  documented response/error envelope shapes.

### Fixed

- `/api/v1/statistics.events.by_type` now reports real categories instead
  of all-`unknown` (the SQL was reading the wrong JSON path).
- `treasury.sync_status.updated_at` now bumps on idle polls so
  `/api/v1/statistics` reflects a live heartbeat
  ([`KI-SY-01`](known-issues.md#ki-sy-01--treasurysync_statusupdated_at-doesnt-bump-on-idle-ticks)).

## Pipeline / data-quality changes (no API shape impact)

These shipped alongside or just before v1.1.0. They affect *what data*
the API serves, not the response shape.

- **Multi-key vendor datum parser** — `parse_vendor_contract_datum` now
  handles the `UTXO-*` family's two-party vendor info constructor. Closes
  [`KI-VND-01`](known-issues.md), unblocks [`KI-MIL-01`](known-issues.md).
- **Milestone-id ordinal normalisation** — when a complete/withdraw event
  uses `MS-N` (1-indexed) but the fund used `m-N` (0-indexed) for the same
  project (or vice versa), the lookup now matches by canonical
  `milestone_order`. Closes [`KI-OC-01`](known-issues.md).
- **`treasury.utxo_history` + Postgres triggers** — every script-address
  UTXO that YACI Store inserts is captured synchronously into a permanent
  history table before pruning can run. Resolves the cold-replay
  limitations [`KI-VND-04`](known-issues.md),
  [`KI-EVT-01`](known-issues.md), [`KI-CR-01`](known-issues.md). Caveat:
  the trigger only protects from the moment it's armed, so to recover
  pre-existing pruned data you need a full YACI Store re-sync.
- **Label fallback for `UTXO-*` milestones** — when a milestone's metadata
  has no `acceptanceCriteria`, the label now falls back to the first line
  of `description`.
- **Documentation** — new [`docs/known-issues.md`](known-issues.md) index
  with stable IDs, repro SQL, and live counts. Existing docs (`README`,
  `api/README`, `database/README`, `docs/architecture.md`,
  `indexer/SETUP.md`, `CLAUDE.md`) refreshed for the post-redesign reality.

## Earlier history

This file starts at v1.1.0. For commits prior to that, see `git log`. The
big pre-1.1 milestones were:

- **Milestone-event silent-drop fix** — restructured `process_complete`
  and `process_withdraw` so every on-chain TOM event is recorded in
  `treasury.events`, even when chain-trace fails. Brought local event
  parity from 55/378 to full coverage versus the deployed feed.
- **Milestone lifecycle redesign** — 4 independent boolean flags
  (`evidence_provided`, `withdrawn`, `paused`, `archived`) plus archive
  model via `superseded_by`.
- **Disburse `destination` JSONB** — column type changed from `TEXT` to
  `JSONB` so the TOM `{ label, details }` object is preserved. (Listed
  again under v1.1.0 Breaking for visibility.)
