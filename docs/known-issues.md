# Known issues — data quality and behavioural quirks

> **Last refreshed:** 2026-05-01 against commit `097c5d3`.
> Counts come from the local DB after a clean sync; rerun the per-entry repro
> SQL for fresh numbers. Production parity is checked via
> `bash scripts/compare_events.sh` (target: `0 deployed-only`).
>
> **Recent resolutions:** the historical-UTXO trigger (KI-UTX-01),
> multi-key datum parsing (KI-VND-01 / KI-MIL-01), milestone-id ordinal
> normalisation (KI-OC-01), and the `vendor_name` / `contract_url` column
> drops (KI-VND-02 / KI-VND-03) all landed in this revision. Entries
> below show their post-fix state.

## How to use this doc

- Each entry has a stable ID (`KI-<area>-<n>`) referenced from PRs and issues.
- "Repro query" runs as-is against the local Postgres
  (`postgresql://postgres:postgres@localhost:5433/administration_data`).
- "Current count" is point-in-time at the date above.
- Entries are split into:
  - **Section A — NULL fields** (the data-quality holes)
  - **Section B — On-chain inconsistencies** (chain data the code can't fully reconcile)
  - **Section C — Cold-replay limitation** (UTXO pruning during fresh local sync)
  - **Section D — Sync-loop quirks** (operational gotchas)
  - **Section E — Spec/code mismatches**

When opening an issue, cite the ID. When fixing one, remove the entry (or
update its count to zero) in the same PR.

---

## Section A — NULL fields

### A.1 `treasury.treasury_contracts`

All five nullable columns are populated for the only treasury we currently
track. No active anomalies — listed for completeness because the schema
allows NULL.

| Column | When NULL is expected | When NULL is anomalous |
|---|---|---|
| `contract_address`, `stake_credential` | Before `initialize` event | Initialize ran but neither `yaci_store.address_utxo` nor `treasury.utxos` had the script output (`event_processor.rs:169`) |
| `publish_tx_hash`, `publish_time` | Treasury never published on chain | A publish event was received but didn't write — investigate |
| `initialized_tx_hash`, `initialized_at` | Treasury never initialized | Same as above for initialize |
| `permissions` | Publish metadata didn't include the field | Publish metadata included it but extraction failed |

**Repro query**

```sql
SELECT id, contract_instance,
       contract_address IS NULL AS missing_addr,
       publish_tx_hash IS NULL AS missing_publish,
       initialized_tx_hash IS NULL AS missing_init,
       permissions IS NULL AS missing_perms
FROM treasury.treasury_contracts;
```

**Current count:** 0 anomalous NULLs across 1 row.

---

### A.2 `treasury.vendor_contracts`

#### KI-VND-01 — `vendor_payment_key_hash` NULL when datum parse fails *(RESOLVED)*
- **Resolved by:** parser rewrite in `api/src/parsers/datum.rs::parse_vendor_contract_datum`.
  The vendor-info field is now traversed by `collect_key_hashes`, which walks
  the subtree and gathers every 28-byte `BoundedBytes` (Cardano key-hash size).
  Single-key (`m-N`) format yields one hash; the multi-party
  `Constr(1, [(Constr(0, [bytes]), Constr(N, [bytes]))])` format used by the
  `UTXO-*` projects yields multiple hashes joined with `,`.
- **Status:** all `UTXO-*` projects now record their key hashes after a fresh
  sync. Run the repro to confirm.

**Repro query**

```sql
SELECT project_id, fund_tx_hash
FROM treasury.vendor_contracts
WHERE vendor_payment_key_hash IS NULL
ORDER BY project_id;
```

**Current count:** 10 / 42 vendor contracts.

#### KI-VND-02 — `vendor_name` (deprecated) *(RESOLVED)*
- Column dropped from `treasury.vendor_contracts`, models, routes and views.

#### KI-VND-03 — `contract_url` (deprecated) *(RESOLVED)*
- Column dropped from `treasury.vendor_contracts`, models, routes and views.

#### KI-VND-04 — `contract_address` NULL on cold replay *(RESOLVED)*
- **Resolved by:** the `treasury.utxo_history` table + Postgres trigger on
  `yaci_store.address_utxo` (see KI-UTX-01). Every script-address UTXO is
  now captured synchronously inside YACI Store's INSERT, so pruning never
  has a chance to wipe it before we read it.

**Repro query**

```sql
SELECT project_id, fund_tx_hash
FROM treasury.vendor_contracts
WHERE contract_address IS NULL;
```

---

### A.3 `treasury.milestones`

#### KI-MIL-01 — `amount_lovelace` / `time_limit` / `label` / `acceptance_criteria` NULL together *(RESOLVED for the parser part)*
- **Resolved by:** the same datum-parser rewrite as KI-VND-01. The
  `UTXO-*` projects now decode correctly so milestones get
  `amount_lovelace`, `time_limit`, and `paused` populated.
- **Note:** `label` and `acceptance_criteria` come from the metadata fields
  rather than the datum, so they're populated independently. The label
  extractor (`extract_milestone_label_description`,
  `event_processor.rs:1419`) now falls back to the first line of
  `description` when `acceptanceCriteria` is missing — fixes label NULLs
  for `UTXO-*` projects whose metadata uses `description` only.

The original analysis is preserved below for context.


- **Why correlated:** all four come from the fund event metadata + datum.
  Projects whose fund metadata uses a milestones-array with bare descriptions
  (no `label`, no `acceptanceCriteria`, no `amount`, no parseable datum)
  produce milestones where every derived field is NULL except `description`
  (which comes from the metadata field directly via
  `extract_text_from_value`).
- **Currently affected:** 136 / 386 active milestones (35%) — entirely
  concentrated in the 10 `UTXO-*` projects from KI-VND-01:

  | project_id | NULL `label` count | total milestones |
  |---|---:|---:|
  | UTXO-EC-0002-25-05 | 21 | 21 |
  | UTXO-EC-0002-25-06 | 21 | 21 |
  | UTXO-EC-0002-25-03 | 20 | 20 |
  | UTXO-EC-0002-25-02 | 19 | 19 |
  | UTXO-EC-0002-25-04 | 18 | 18 |
  | UTXO-EC-0002-25-01 | 16 | 16 |
  | UTXO-EC-0003-25 | 8 | 8 |
  | UTXO-EMI-0001-25 | 5 | 5 |
  | UTXO-EG-0003-25 | 4 | 4 |
  | UTXO-ER-0001-25 | 4 | 4 |

- **Code path:** `process_fund` → `extract_milestone_label_description`
  (`event_processor.rs:1403`) for label/AC; datum parse for amount/time_limit.
- **Likely root cause:** these projects use `MS-N` milestone identifiers
  with a different datum format that `parse_vendor_contract_datum` doesn't
  yet handle (see KI-OC-01).

**Repro query**

```sql
SELECT vc.project_id, COUNT(*) AS missing_label
FROM treasury.milestones m
JOIN treasury.vendor_contracts vc ON vc.id = m.vendor_contract_id
WHERE NOT m.archived AND m.label IS NULL
GROUP BY vc.project_id ORDER BY 2 DESC;
```

#### KI-MIL-02 — `withdraw_*` / `complete_*` / `archived_*` columns
- All conditional on the corresponding boolean flag being true. No anomalies
  observed (`withdrawn=TRUE` rows always have non-NULL `withdraw_*`).

**Repro query**

```sql
SELECT COUNT(*) FILTER (WHERE withdrawn AND withdraw_tx_hash IS NULL) AS withdrawn_no_tx,
       COUNT(*) FILTER (WHERE evidence_provided AND complete_tx_hash IS NULL) AS evidenced_no_tx,
       COUNT(*) FILTER (WHERE archived AND archived_by_tx_hash IS NULL) AS archived_no_tx
FROM treasury.milestones;
```

**Current count:** 0, 0, 0.

---

### A.4 `treasury.events`

#### KI-EVT-01 — `vendor_contract_id` NULL on chain-trace failure *(RESOLVED)*
- **Resolved by:** historical-UTXO trigger (KI-UTX-01). Chain-trace inputs
  are now reliably present in `treasury.utxo_history` regardless of pruning,
  so the trace finds the seed for every event whose ancestor is a fund tx
  we've processed.
- See the analysis below for context.


- **Code path:** every milestone-level handler (`process_complete:511`,
  `process_withdraw:626`, `process_pause:721`, `process_resume:764`) calls
  `find_vendor_contract_from_inputs` (`event_processor.rs:1047`) when
  `body.identifier` is empty. If the trace returns `None`, the event is
  inserted with `vendor_contract_id = NULL` (intentional — see fix history;
  events are never silently dropped).
- **Why it happens:** input UTXOs for these txs were pruned from
  `yaci_store.address_utxo` before the local sync caught up, and no `fund`
  output we processed seeded them either.
- **Currently affected:**

  | event_type | NULL vc | total | % |
  |---|---:|---:|---:|
  | complete | 30 | 188 | 16% |
  | withdraw | 9 | 126 | 7% |
  | pause | 10 | 63 | 16% |
  | resume | 7 | 32 | 22% |
  | **total** | **56** | **409** | **14%** |

  Treasury-level events (publish, initialize, disburse) have NULL `vc` by
  design — they aren't tied to a vendor contract.

**Repro query**

```sql
SELECT event_type,
       COUNT(*) FILTER (WHERE vendor_contract_id IS NULL) AS null_vc,
       COUNT(*) AS total
FROM treasury.events
WHERE event_type IN ('complete','withdraw','pause','resume')
GROUP BY 1 ORDER BY 1;
```

#### KI-EVT-03 — fund/initialize/etc have NULL `milestone_id` by design
- Treasury- and contract-level events aren't tied to a single milestone;
  schema permits NULL. Listed only because grouped count looks suspicious
  at first glance.

**Repro query**

```sql
SELECT event_type, COUNT(*) FILTER (WHERE milestone_id IS NULL) AS null_milestone
FROM treasury.events GROUP BY 1 ORDER BY 1;
```

---

### A.5 `treasury.utxos`

#### KI-UTX-01 — `treasury.utxo_history` table + Postgres trigger *(IMPLEMENTED)*
- **Implementation:** `install_utxo_history_triggers`
  (`api/src/services/sync.rs`) creates two triggers at API startup:
  - `capture_address_utxo` AFTER INSERT/UPDATE on `yaci_store.address_utxo`
    copies every `addr1x*` row into `treasury.utxo_history`.
  - `mark_utxo_spent` AFTER INSERT on `yaci_store.tx_input` flags the
    corresponding `treasury.utxo_history` row as spent.
- **Outcome:** complete UTXO history at script addresses is preserved
  regardless of YACI Store's pruning window. Resolves KI-VND-04, KI-EVT-01,
  KI-CR-01.

#### KI-UTX-02 — `vendor_contract_id` IS NULL on non-script UTXOs (by design)
- **Why:** `pre_fetch_utxos` (`event_processor.rs:1209`) inserts every output
  of every TOM-event tx without `vendor_contract_id`. The chain-trace seed
  (set later by `process_fund` and `find_vendor_contract_from_inputs`) only
  fills it for outputs at the script address. Non-script change/fee outputs
  remain NULL by design — they aren't part of the chain.
- **Currently affected:** 767 / 1222 rows. Not anomalous — expected.

**Repro query**

```sql
SELECT
  CASE
    WHEN vendor_contract_id IS NOT NULL AND address IS NOT NULL THEN 'fully_tracked'
    WHEN vendor_contract_id IS NULL AND address IS NOT NULL THEN 'address_only'
    WHEN vendor_contract_id IS NULL AND address IS NULL THEN 'sparse'
    ELSE 'other'
  END AS state,
  COUNT(*) AS count
FROM treasury.utxos GROUP BY 1 ORDER BY 2 DESC;
```

**Current breakdown:** `address_only=767`, `fully_tracked=455`.

#### KI-UTX-02 — 5 rows with NULL `lovelace_amount`
- Output recorded but `yaci_store.address_utxo` had no row when looked up
  (probably collateral / null-value outputs).
- **Currently affected:** 5 / 1222.

**Repro query**

```sql
SELECT tx_hash, output_index, address FROM treasury.utxos WHERE lovelace_amount IS NULL;
```

---

## Section B — On-chain data inconsistencies

### KI-OC-01 — Milestone-id naming drift (`m-N` vs `MS-N`) *(RESOLVED at lookup time)*
- **Resolved by:** `canonical_milestone_order`
  (`api/src/services/event_processor.rs`) parses metadata keys to a 1-indexed
  `milestone_order` (`m-N` → `N+1`, `MS-N` → `N`). `process_complete` and
  `process_withdraw` UPDATE clauses now match `milestone_id = $key OR
  milestone_order = $order`, so events whose metadata key uses the opposite
  scheme to the fund event still resolve.
- Stored `milestone_id` is left as-is.

The original analysis is preserved below.


- **Pattern:** fund events for some projects emit milestones as an array
  whose elements have `identifier: "m-N"`; fund events for the `UTXO-*`
  family emit them as `"MS-N"`. Our parser stores whatever the
  `identifier` field says.
- **Indexing convention:** the two schemes use different bases —
  `m-N` is **0-indexed** (`m-0`, `m-1`, …, `m-{count-1}`) while `MS-N`
  is **1-indexed** (`MS-1`, `MS-2`, …, `MS-{count}`). So the *first*
  milestone of a project is `m-0` under one convention and `MS-1` under
  the other; positionally they are the same milestone. A future
  normaliser that wants to merge the two formats can use this offset.
- **Effect on complete events:** of 188 complete events, 107 use `m-N` keys
  and 81 use `MS-N` keys. After the disambiguation hint to
  `find_vendor_contract_from_inputs`, this no longer causes silent event
  drops (every event lands in `treasury.events`), but it surfaces as
  KI-VND-01 / KI-MIL-01 because the same projects have a different datum
  format the parser can't handle.

**Repro query**

```sql
WITH cmp AS (
  SELECT body::jsonb -> 'body' -> 'milestones' AS ms_field
  FROM yaci_store.transaction_metadata
  WHERE label='1694' AND body::jsonb->'body'->>'event'='complete'
)
SELECT
  COUNT(*) FILTER (WHERE k LIKE 'm-%') AS m_dash,
  COUNT(*) FILTER (WHERE k LIKE 'MS-%') AS ms_dash,
  COUNT(*) FILTER (WHERE k NOT LIKE 'm-%' AND k NOT LIKE 'MS-%') AS other
FROM cmp, jsonb_object_keys(ms_field) k
WHERE jsonb_typeof(ms_field) = 'object';
```

### KI-OC-02 — `body.identifier` empty on every milestone-level event
- 100% of `complete`, `withdraw`, `pause`, `resume` on-chain events have an
  empty top-level `identifier`, so the cheap project lookup is never
  available — every such event must chain-trace. This is what makes
  KI-EVT-01 visible at all.
- **Currently affected:** complete 188/188, withdraw 126/126, pause 63/63,
  resume 32/32 — 100% across the board.

**Repro query**

```sql
SELECT body::jsonb->'body'->>'event' AS event_type,
       COUNT(*) FILTER (WHERE COALESCE(body::jsonb->'body'->>'identifier','') = '') AS empty_id,
       COUNT(*) AS total
FROM yaci_store.transaction_metadata
WHERE label='1694' AND body::jsonb->'body'->>'event' IN ('complete','withdraw','pause','resume')
GROUP BY 1 ORDER BY 1;
```

### KI-OC-03 — Multi-input txs with sibling-project fee inputs
- A single complete/withdraw tx can take fee/collateral inputs from another
  project's UTXO chain. Without disambiguation, the older code attributed
  the event to whichever project's input came first.
- **Mitigation in code:** `find_vendor_contract_from_inputs`
  (`event_processor.rs:1047`) now scores candidate vendor_contract_ids
  against `body.milestones` keys and prefers the one whose stored milestones
  match (`collect_milestone_id_hints` at `:1450`).
- **Currently affected:** observable indirectly via KI-EVT-02 = 0.

---

## Section C — Cold-replay UTXO-pruning limitation

### KI-CR-01 — Fresh local sync can't reconstruct fully-pruned chains *(MITIGATED)*
- **Mitigated by:** the historical-UTXO trigger (KI-UTX-01). Going forward,
  every UTXO YACI Store inserts is captured in `treasury.utxo_history` before
  it can be pruned, so continuous-operation chain trace works fully.
- **Caveat:** the trigger only protects UTXOs *from the moment it was
  installed*. If `yaci_store.address_utxo` had already been pruned before
  the trigger was armed (typical local install), historical fund-output
  datums may still be missing from `treasury.utxo_history`. To recover
  those, wipe both schemas and re-sync from
  `STORE_CARDANO_SYNC_START_SLOT`:
  ```bash
  ./dev.sh stop
  docker volume rm administration-data_postgres_data
  ./dev.sh start
  ```
  Triggers must already be present in `database/init/02-treasury-schema.sql`
  *or* the API must arm them before YACI Store finishes its initial sync.
  The `install_utxo_history_triggers` startup hook in
  `api/src/services/sync.rs` runs early enough on a fresh install to
  satisfy this.

---

## Section D — Sync-loop quirks

### KI-SY-01 — `treasury.sync_status.updated_at` doesn't bump on idle ticks *(RESOLVED)*
- **Resolved by:** `sync_new_events` (`api/src/services/sync.rs`) now bumps
  `updated_at` on the `rows.is_empty()` path so `/api/v1/statistics`
  reflects a live heartbeat even when no new TOM events have arrived.

### KI-SY-02 — `last_slot` can advance past failed events on connection reset
- **Symptom observed:** during a postgres restart mid-batch
  (2026-04-28), 5 events failed to insert. Continuous-sync logged
  `Sync error: error communicating with database` then advanced `last_slot`
  past those events on the next successful batch, so they were never
  retried.
- **Why:** `sync.rs` `for row in rows { if Err { continue } else { last_slot
  = row.slot } }` — the success of later events bumps the watermark past
  the failed ones.
- **Recovery:** restart the API; `sync_all_events` re-processes from the
  beginning. All inserts are idempotent via `ON CONFLICT (tx_hash) DO UPDATE`.
- **Fix candidate:** track `last_slot` as the slot of the LAST contiguous
  success (don't advance past holes), or run `sync_all_events` periodically.

---

## Section E — Spec / code mismatches

### KI-API-01 — `disburse.destination` typed as string instead of `{label, details}` *(RESOLVED)*
- **Resolved by:** `treasury.events.destination` is now `JSONB`. `process_disburse`
  preserves the full TOM `{label, details}` object instead of flattening to a
  string. API model fields updated to `serde_json::Value`. **Breaking change**
  for downstream consumers that previously read `destination` as a string —
  they should now read `destination.label`.

---

## Index summary

| ID | Area | Status |
|---|---|---|
| KI-VND-01 | datum parse failure on `UTXO-*` projects | **resolved** (multi-key parser) |
| KI-VND-02 | `vendor_name` deprecated | **resolved** (column dropped) |
| KI-VND-03 | `contract_url` deprecated | **resolved** (column dropped) |
| KI-VND-04 | `contract_address` NULL on cold replay | **resolved** (utxo_history trigger) |
| KI-MIL-01 | NULL `label`/`amount`/`time_limit`/`AC` for `UTXO-*` | **resolved** for datum-derived fields |
| KI-EVT-01 | NULL `vendor_contract_id` on chain-trace failure | **resolved** (utxo_history trigger) |
| KI-EVT-03 | NULL `milestone_id` on treasury-level events | by design |
| KI-UTX-01 | historical-UTXO table + trigger | **implemented** |
| KI-UTX-02 | `vendor_contract_id` NULL on non-script UTXOs | by design |
| KI-OC-01 | milestone-id naming drift (m-N vs MS-N) | **resolved at lookup time** |
| KI-OC-02 | empty `body.identifier` everywhere | on-chain limitation |
| KI-OC-03 | multi-input sibling-project txs | resolved (disambiguation hint) |
| KI-CR-01 | cold-replay limitation | **resolved** (utxo_history trigger) |
| KI-SY-01 | idle `updated_at` doesn't bump | **resolved** |
| KI-SY-02 | `last_slot` advances past failed events | open |
| KI-API-01 | `destination` schema mismatch | **resolved** (JSONB; breaking API change) |
