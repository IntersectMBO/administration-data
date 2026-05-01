# Known issues — data quality and behavioural quirks

> **Last refreshed:** 2026-05-01 against commit `ea0ac13`, mainnet sync at block
> 13,354,543 / slot 185,915,933.
> Counts come from the local DB after a clean sync; rerun the per-entry repro
> SQL for fresh numbers. Production parity is checked via
> `bash scripts/compare_events.sh` (target: `0 deployed-only`).

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

#### KI-VND-01 — `vendor_payment_key_hash` NULL when datum parse fails
- **Code path:** `process_fund` calls `parse_vendor_contract_datum`
  (`api/src/parsers/datum.rs:42`) and only updates the column on `Ok(_)`
  (`event_processor.rs:436-442`). On parse error the warn is logged and the
  column stays NULL.
- **Currently affected:** all 10 `UTXO-*` projects (24% of vendor contracts)
  — the `UTXO-EC-0002-25-*` family, `UTXO-EC-0003-25`, `UTXO-EG-0003-25`,
  `UTXO-EMI-0001-25`, `UTXO-ER-0001-25`. Strongly correlates with
  KI-OC-01 / KI-MIL-01: these projects use a different milestone-id and
  datum convention that the current parser doesn't handle.

**Repro query**

```sql
SELECT project_id, fund_tx_hash
FROM treasury.vendor_contracts
WHERE vendor_payment_key_hash IS NULL
ORDER BY project_id;
```

**Current count:** 10 / 42 vendor contracts.

#### KI-VND-02 — `vendor_name` always NULL (deprecated)
- **Why:** the TOM spec has no `vendor.name` field; the doc string at
  `database/schema/treasury.sql:36` and `docs/event-processing.md:515-518`
  marks it deprecated. `process_fund` never writes to it.
- **Currently affected:** 100% (42 / 42).
- **Action:** consider dropping the column in a follow-up migration.

#### KI-VND-03 — `contract_url` always NULL (deprecated)
- Same shape as KI-VND-02. No on-chain field maps to it.
- **Currently affected:** 100% (42 / 42).

#### KI-VND-04 — `contract_address` NULL on cold replay
- **Code path:** `get_script_utxo_for_tx` (`event_processor.rs:1213`) tries
  `yaci_store.address_utxo`, then falls back to `treasury.utxos`. If both
  miss (UTXO already pruned and pre-fetch never captured it), NULL.
- **Currently affected:** 0 / 42 — local DB has been kept warm.
- **Cross-ref:** KI-CR-01 (cold-replay limitation).

**Repro query**

```sql
SELECT project_id, fund_tx_hash
FROM treasury.vendor_contracts
WHERE contract_address IS NULL;
```

---

### A.3 `treasury.milestones`

#### KI-MIL-01 — `amount_lovelace` / `time_limit` / `label` / `acceptance_criteria` NULL together
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

#### KI-EVT-01 — `vendor_contract_id` NULL on chain-trace failure
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

#### KI-EVT-02 — `milestone_id` NULL when metadata keys don't match stored ids
- **Code path:** `process_complete:521-562`, `process_withdraw:649-695`.
  After the milestone-key disambiguation hint was added to
  `find_vendor_contract_from_inputs`, all currently-recorded events whose
  `vendor_contract_id` resolved have a matching milestone too.
- **Currently affected:** 0 events (where `vc IS NOT NULL AND milestone_id IS NULL`).

**Repro query**

```sql
SELECT COUNT(*) FROM treasury.events
WHERE event_type IN ('complete','withdraw')
  AND vendor_contract_id IS NOT NULL
  AND milestone_id IS NULL;
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

#### KI-EVT-04 — `amount_lovelace` NULL on withdraw with zero non-script outputs
- **Code path:** `process_withdraw:622-650`. Sums `lovelace_amount` of
  non-`addr1x*` outputs from `yaci_store.address_utxo`, then falls back to
  `treasury.utxos`. If both return 0 → NULL.
- **Currently affected:** 0 / 126 withdraw events (every recorded withdraw
  has a non-zero amount).

---

### A.5 `treasury.utxos`

#### KI-UTX-01 — 767 / 1222 rows with `vendor_contract_id IS NULL`
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

### KI-OC-01 — Milestone-id naming drift (`m-N` vs `MS-N`)
- **Pattern:** fund events for some projects emit milestones as an array
  whose elements have `identifier: "m-N"`; fund events for the `UTXO-*`
  family emit milestones as an array of `MS-N` identifiers, but our parser
  stores them as `m-N`/`MS-N` based on whatever the `identifier` field says.
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

### KI-CR-01 — Fresh local sync can't reconstruct fully-pruned chains
- **Symptom:** the 56 events under KI-EVT-01 with NULL `vendor_contract_id`.
- **Why:** YACI Store prunes spent UTXOs after ~2160 blocks (~10 days). On a
  cold replay from an old `STORE_CARDANO_SYNC_START_SLOT`, by the time the
  indexer is processing event N, its input UTXOs from event N-K may already
  be gone. `pre_fetch_utxos` (`event_processor.rs:1209`) cannot capture what
  YACI Store no longer has.
- **Mitigation 1:** keep the API running continuously — it captures outputs
  before pruning gets a chance.
- **Mitigation 2:** set `STORE_CARDANO_SYNC_START_SLOT` to a more recent
  block so less history needs to be reconstructed.
- **Cross-ref:** the gotcha in `CLAUDE.md` under "Gotchas".

---

## Section D — Sync-loop quirks

### KI-SY-01 — `treasury.sync_status.updated_at` doesn't bump on idle ticks
- **Code:** `sync_new_events` in `api/src/services/sync.rs` returns early
  when `rows.is_empty()` and never UPDATEs. `/api/v1/statistics`'s
  `last_updated` looks "stale" even when the loop is alive and polling
  every 15 s.
- **Workaround:** trust the indexer cursor for liveness, or check
  `lsof -i :8080`.
- **Fix candidate:** bump `updated_at` (and optionally write the indexer's
  current slot) on every poll, even when no work is found.

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

### KI-API-01 — `disburse.destination` typed as string instead of `{label, details}`
- **Spec:** `docs/event-processing.md` line ~286 documents `body.destination`
  as an object `{label, details}`.
- **Code:** `process_disburse` (`event_processor.rs:583`) extracts via
  `extract_text` and stores the resulting string in
  `treasury.events.destination`. The `details` sub-field is dropped.
- **Currently affected:** all 4 `disburse` events in the DB.
- **Fix candidate:** change `treasury.events.destination` to `JSONB`, keep
  the full object, expose both fields in the API response.

---

## Index summary

| ID | Area | Severity | Status |
|---|---|---|---|
| KI-VND-01 | datum parse failure on `UTXO-*` projects | High — breaks downstream KI-MIL-01 | open |
| KI-VND-02 | `vendor_name` deprecated, always NULL | Cleanup | open |
| KI-VND-03 | `contract_url` deprecated, always NULL | Cleanup | open |
| KI-VND-04 | `contract_address` NULL on cold replay | Acceptable; mitigation documented | open |
| KI-MIL-01 | NULL `label`/`amount`/`time_limit`/`AC` for `UTXO-*` | High — same root as VND-01 | open |
| KI-EVT-01 | NULL `vendor_contract_id` on chain-trace failure | Medium — cold-replay only | open |
| KI-UTX-01 | NULL `vendor_contract_id` on non-script UTXOs | By design | not a bug |
| KI-OC-01 | milestone-id naming drift | Documented; mitigated by disambiguation | open |
| KI-OC-02 | empty `body.identifier` everywhere | On-chain data; can't fix locally | not a bug |
| KI-OC-03 | multi-input sibling-project txs | Mitigated | resolved |
| KI-CR-01 | cold-replay limitation | Documented mitigation | open |
| KI-SY-01 | idle `updated_at` doesn't bump | Low — UX confusion | open |
| KI-SY-02 | `last_slot` advances past failed events | Medium — recoverable via restart | open |
| KI-API-01 | `destination` schema mismatch | Low — data loss for one field | open |
