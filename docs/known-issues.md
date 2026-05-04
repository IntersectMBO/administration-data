# Known issues — data quality and behavioural quirks

> **Last refreshed:** 2026-05-03 (post six-bug-cascade fix + true cold
> resync) against commit `6db1581`+. Rerun the per-entry repro SQL for
> fresh numbers.
>
> **Verified resolved by cold resync:**
> - **KI-VND-01** — `vendor_payment_key_hash` NULL: **10/42 → 0/42**
> - **KI-MIL-01 (datum-derived)** — NULL `amount_lovelace`/`time_limit`:
>   **136/386 → 16/364** (all 16 are KI-MOD-01 modify-event milestones).
> - **KI-VND-05** — corrupted utxo_history datums: **resolved** by cold
>   resync + the merged-source `get_script_utxo_for_tx` query (bug #6).
> - **KI-EVT-01-residual** — 12/413 events still NULL, all clustered on
>   the 2 KI-MOD-01-affected projects' descendants (slight regression
>   from 4 pre-cold-resync — likely milestone-id-hint ambiguity in
>   `find_project_from_inputs` when modify events introduce new IDs).
> - **KI-SY-02** — Phase 1 (contiguous-success watermark) shipped.
> - **KI-VND-04**, **KI-CR-01**, **KI-UTX-03** — confirmed clean.
>
> **Six distinct bugs were the cause of the KI-VND-01 cascade** (not a
> single parser-strictness defect):
> 1. Sync race during cold catch-up — `sync_all_events` only ran once at
>    startup, racing yaci_store's address_utxo ingestion. **Fix:**
>    periodic 10-min `sync_all_events` task.
> 2. `vendor_payment_key_hash VARCHAR(56)` rejected the 113-char
>    multi-key hash. **Fix:** widened to `TEXT`.
> 3. `get_script_utxo_for_tx` LIMIT 1 with no ORDER BY picked a tiny
>    `d87980` reference output instead of the kilobyte project datum.
>    **Fix:** ORDER BY length DESC.
> 4. `process_fund` blindly overwrote a good captured datum with the bad
>    one when bug #3 fired. **Fix:** preserve-larger-datum guard.
> 5. `process_fund`'s milestone-update filtered `NOT withdrawn`, breaking
>    index alignment with the fund datum on re-runs. **Fix:** removed.
> 6. `get_script_utxo_for_tx` queried yaci_store first and never fell
>    back to `treasury.utxo_history` even when the latter had a longer
>    captured datum (only surfaces post-resync because some funds have
>    a *spent-and-pruned* vendor-contract output and an *unspent*
>    treasury reference output — yaci_store retains the small one).
>    **Fix:** UNION ALL across both sources, ORDER BY length DESC.
>
> **Schema refactor note:** project-level columns moved from
> `treasury.vendor_contracts` to `treasury.projects`. Milestones and
> events FK via `project_db_id`. `treasury.utxos` removed in favour of
> `treasury.utxo_history`.
>
> **Still open:** KI-OC-02 (on-chain limitation, can't fix), KI-MOD-01
> (modify events don't reflect updated milestone amounts / time limits in
> the API — new milestone rows ship with NULL datum-derived fields), small
> KI-EVT-01 regression on KI-MOD-01-affected projects (12 NULL events),
> KI-FIN-04 (per-project balance under-counts the raw PSSC total when
> chain trace can't attribute every UTXO).

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
| `contract_address`, `stake_credential` | Before `initialize` event | Initialize ran but neither `yaci_store.address_utxo` nor `treasury.utxo_history` had the script output (`process_initialize` in `event_processor.rs`) |
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

### A.2 `treasury.projects` (formerly project-level cols on `vendor_contracts`)

#### KI-VND-01 — `vendor_payment_key_hash` NULL on `UTXO-*` projects *(RESOLVED — six-bug cascade, verified post cold resync)*

The original "10/42 NULL" symptom turned out to be a cascade of six
separate bugs, not the single parser-strictness defect the previous
analysis suspected. The parser was always correct on real CBOR (proven
by four fixture tests in `api/src/parsers/datum.rs`); a postgres
column-width error in the `UPDATE` step was being swallowed by an
all-or-nothing `match` and a misleading DEBUG-level log.

After the cold resync verified all six fixes work end-to-end:
**0/42 NULL key hashes, 0 parse errors.**

##### The six bugs and their fixes

1. **Sync race during cold catch-up** — `sync_all_events` ran once at
   API startup. During catch-up, `yaci_store.transaction_metadata` was
   visible to the API before the matching `address_utxo` row, so
   `get_script_utxo_for_tx` returned `None` at fund-time and the datum
   lookup never happened. Once yaci_store caught up, the trigger
   captured the datum into `treasury.utxo_history` — but `process_fund`
   wasn't re-run.
   **Fix:** added a separate `tokio::spawn` task in `sync.rs` running
   `sync_all_events` every 10 minutes. The idempotent `ON CONFLICT DO
   UPDATE` chain backfills as soon as yaci_store catches up.
2. **`vendor_payment_key_hash` column too narrow** — `VARCHAR(56)`
   rejected the 113-char joined multi-key hash from UTXO-* projects
   (`hash1,hash2`) with `value too long for type character varying(56)`.
   The all-or-nothing `match parse_project_datum() { Ok => …; Err =>
   debug!(…) }` swallowed the error.
   **Fix:** widened to `TEXT` in `database/schema/treasury.sql`,
   `database/init/02-treasury-schema.sql`, plus an `ALTER TABLE` against
   the live DB.
3. **`get_script_utxo_for_tx` picked the wrong UTXO** — `LIKE 'addr1x%'
   LIMIT 1` with no `ORDER BY` could return the change/treasury output
   carrying an empty `Constr(0, [])` datum (`d87980`, 3 bytes) instead
   of the vendor-contract output with the actual project datum
   (kilobytes). On fund txs that produce two `addr1x*` outputs (vendor
   contract + treasury change), this was nondeterministic.
   **Fix:** `ORDER BY length(COALESCE(inline_datum, '')) DESC` — the
   largest datum reliably points to the vendor contract output.
4. **`process_fund` overwrote the captured datum** — when bug #3 fired,
   the `UPDATE treasury.utxo_history SET inline_datum_cbor = $1` at the
   end of `process_fund` blindly wrote the bad 3-byte datum over a
   previously-captured 1.3kB good datum. Once corrupted and yaci_store
   pruned the source, recovery required a true cold resync.
   **Fix:** `WHERE inline_datum_cbor IS NULL OR length($1) >
   length(inline_datum_cbor)` — only overwrite with a *better* datum.
5. **`process_fund` filtered `NOT withdrawn`** — the milestone-update
   loop selected only non-withdrawn rows. Fine on first run, but on a
   periodic re-run after some milestones became withdrawn, index
   alignment between the (full, fund-time) datum array and the
   (filtered, current-state) DB rows broke, leaving the now-withdrawn
   milestones permanently NULL.
   **Fix:** removed `AND NOT withdrawn` from the select. The fund tx's
   datum represents *initial* state; we always update by
   `milestone_order` regardless of current withdrawn flag.
6. **`get_script_utxo_for_tx` never preferred `utxo_history` over
   yaci_store when both had a row** — the yaci_store query ran first
   and returned whatever was there. For some fund txs (e.g.
   `b39d013c…`, `5bc5a75e…`) the *vendor-contract* output was spent and
   pruned from yaci_store but captured by the trigger into
   utxo_history; the *unspent* treasury-reference output (`d87980`,
   3 bytes) survived in yaci_store. Result: yaci_store returned the
   trivial datum and we never consulted utxo_history's real one.
   This bug was invisible on the first cold-resync test because the
   spent-and-pruned outputs were re-fetched while the trigger was
   armed; it surfaced only when querying for fund-time state of older
   txs after their vendor-contract outputs had since been spent.
   **Fix:** `UNION ALL` yaci_store and utxo_history in a single query,
   `ORDER BY length(datum) DESC LIMIT 1`. Source-agnostic: always picks
   the longer datum across both.

##### Defensive hardening also landed

- **Partial parser** — `parse_project_datum` now returns
  `ParsedProjectDatum { vendor_payment_key_hash: Option<String>,
  vendor_info_error: Option<String>, milestones: Vec<Result<…, String>>,
  top_level_error: Option<String> }` so vendor info persists even when
  individual milestones fail to parse.
- **`datum_parse_error TEXT`** columns added to `treasury.projects` and
  `treasury.milestones` for SQL-queryable diagnostics.
- **`tracing::debug!` → `tracing::warn!`** for parse failures so they
  appear in default logs.
- **Four real-CBOR fixture tests** in `api/src/parsers/datum.rs`
  (UTXO-EMI-0001-25, UTXO-EC-0002-25-01, UTXO-EC-0002-25-03,
  partial-parse smoke test).

##### Verified counts (after cold resync from `STORE_CARDANO_SYNC_START_SLOT`)

| Metric | Before | After |
|---|---:|---:|
| `treasury.projects` NULL `vendor_payment_key_hash` | 10 / 42 | **0 / 42** |
| `treasury.projects.datum_parse_error` set | n/a | **0 / 42** |
| `treasury.milestones` NULL `amount_lovelace` (active) | 136 / 386 | **16 / 364** |

The remaining 16 NULL milestone amounts are all from KI-MOD-01 (modify
events created new milestone rows with new IDs that don't pick up the
new contract output's datum). 8 each on `UTXO-EC-0002-25-03` and
`UTXO-EC-0002-25-04`.

##### Repro queries

```sql
SELECT project_id, fund_tx_hash, datum_parse_error
FROM treasury.projects
WHERE vendor_payment_key_hash IS NULL OR datum_parse_error IS NOT NULL
ORDER BY project_id;

SELECT p.project_id, COUNT(*) AS missing
FROM treasury.milestones m JOIN treasury.projects p ON p.id = m.project_db_id
WHERE NOT m.archived AND m.amount_lovelace IS NULL
GROUP BY p.project_id ORDER BY 2 DESC;
```

#### KI-VND-05 — Datum corruption from prior bug #4 *(RESOLVED — cold resync + bug #6 fix)*
- **Was:** `UTXO-EC-0002-25-03` (fund tx `b39d013c…`) and
  `EC-0013(1,2,7)-25` (fund tx `5bc5a75e…`) had their captured datums
  overwritten with `d87980` (6 bytes) before bug #4 was fixed.
- **Resolution path:** the cold resync from
  `STORE_CARDANO_SYNC_START_SLOT` (with the trigger armed before
  yaci_store ingestion) captured the original kilobyte-scale datums
  into `treasury.utxo_history`. The merged-source query (bug #6 fix in
  `get_script_utxo_for_tx`) ensures the captured datum wins over the
  surviving 3-byte yaci_store reference output.
- **Verified:** `b39d013c…` output 0 = 1320 bytes, `5bc5a75e…` output 0
  = 1414 bytes; both parse to 20 + 23 milestones with no errors.
  `vendor_payment_key_hash` and `datum_parse_error` columns now
  populated/clear on these projects.
- **Operator note:** if a production deployment ran continuously while
  bugs #3 and #4 were active, it may have similarly corrupted datums.
  Check with `SELECT project_id FROM treasury.projects WHERE
  datum_parse_error IS NOT NULL`. Recovery is the same wipe-and-resync.

#### KI-MOD-01 — `modify` events don't update milestone amounts or time limits *(OPEN — TODO)*
- **User-visible symptom:** when an oversight committee submits a `modify`
  event to change a milestone's payout amount or time limit, the API
  continues to surface stale or NULL values for those fields. The on-chain
  contract reflects the new state, but `/api/v1/projects/{id}/milestones`
  doesn't.
- **Pattern:** `process_modify` (`api/src/services/event_processor.rs`)
  archives the existing milestone rows and inserts new ones, then COALESCE-
  updates project naming fields from metadata. It does **not** re-parse the
  modify-tx's output datum, so the new milestone rows' `amount_lovelace` /
  `time_limit` / `paused` fields come out NULL — even when the on-chain
  datum carries the updated values.
- **Currently affected:** 8 active milestones in `UTXO-EC-0002-25-04`
  (IDs MS-5, MS-6, MS-8, MS-9, MS-12, MS-13, MS-17, MS-18 — all created
  by modify events; gaps imply earlier IDs were modified out). The same
  cluster also drives the KI-EVT-01-residual NULL `project_db_id` events.
- **Why this is separate from KI-VND-01:** the fund datum *did* parse
  successfully for these projects; the issue is exclusively in
  `process_modify` which doesn't run the datum-update path that
  `process_fund` does.
- **Proposed fix (small, deferred):** at the end of `process_modify`, look
  up the modify tx's output datum via the same mechanism `process_fund`
  uses (`get_script_utxo_for_tx` + `parse_project_datum`) and run the
  milestone-update loop. Matching by `milestone_order` should align — the
  modified contract's datum reflects current state. Re-running
  `sync_all_events` after the fix lands will backfill via the idempotent
  `ON CONFLICT DO UPDATE` chain; no resync needed.

#### KI-VND-02 — `vendor_name` (deprecated) *(RESOLVED)*
- Column dropped from `treasury.vendor_contracts`, models, routes and views.

#### KI-VND-03 — `contract_url` (deprecated) *(RESOLVED)*
- Column dropped from `treasury.vendor_contracts`, models, routes and views.

#### KI-VND-04 — `contract_address` NULL on cold replay *(RESOLVED — verified by 2026-05-02 cold resync)*
- **Resolved by:** the `treasury.utxo_history` table + Postgres trigger on
  `yaci_store.address_utxo` (see KI-UTX-01). Every script-address UTXO is
  now captured synchronously inside YACI Store's INSERT, so pruning never
  has a chance to wipe it before we read it.
- **Verified:** with the trigger armed before YACI Store ingestion, a fresh
  cold sync from `STORE_CARDANO_SYNC_START_SLOT` produces 0 NULL
  `contract_address` across all 42 projects.

**Repro query**

```sql
SELECT project_id, fund_tx_hash
FROM treasury.projects
WHERE contract_address IS NULL;
```

**Current count:** 0 / 42.

---

### A.3 `treasury.milestones`

#### KI-MIL-01 — milestone field NULLs across the four sub-fields
- **`label`** *(RESOLVED)* — description fallback in
  `extract_milestone_label_description` covers the missing
  `acceptanceCriteria` case. **0 / 364 active milestones NULL.**
- **`amount_lovelace` / `time_limit`** *(LARGELY RESOLVED — see KI-VND-01)* —
  was 136/386, now 16/364:
  - 8 from `UTXO-EC-0002-25-03` (KI-VND-05 corruption)
  - 8 from `UTXO-EC-0002-25-04` (KI-MOD-01 modify-event gap)
- **`acceptance_criteria`** *(NOT A BUG — correct on-chain truth)* — the
  remaining NULLs reflect the actual fund metadata. UTXO-* projects
  emit milestones as `{identifier: "MS-N", description: …}` with no
  `acceptanceCriteria` key (verified against `5849b0ec…`'s
  `transaction_metadata.body`). Leave the column NULL — do not invent
  a fallback.

##### Per-project breakdown (NULL `amount_lovelace`)

  | project_id | NULL count | total milestones |
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

##### Repro query

```sql
SELECT p.project_id, COUNT(*) AS missing
FROM treasury.milestones m
JOIN treasury.projects p ON p.id = m.project_db_id
WHERE NOT m.archived AND m.amount_lovelace IS NULL
GROUP BY p.project_id ORDER BY 2 DESC;
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

#### KI-EVT-01 — `project_db_id` NULL on chain-trace failure *(RESOLVED — verified by 2026-05-02 cold resync)*
- **Resolved by:** historical-UTXO trigger (KI-UTX-01). Chain-trace inputs
  are now reliably present in `treasury.utxo_history` regardless of pruning,
  so the trace finds the seed for every event whose ancestor is a fund tx
  we've processed.
- **Verified:** after a fresh cold resync with the trigger armed from the
  start, NULL counts dropped from 56 / 409 (14%) to **4 / 411 (~1%)**.

  | event_type | NULL `project_db_id` | total | % |
  |---|---:|---:|---:|
  | complete | 2 | 189 | 1.1% |
  | withdraw | 2 | 129 | 1.6% |
  | pause | 0 | 62 | 0% |
  | resume | 0 | 31 | 0% |
  | **total** | **4** | **411** | **1.0%** |

- Treasury-level events (publish, initialize, disburse) have NULL
  `project_db_id` by design — they aren't tied to a project.

#### KI-EVT-01-residual — 12 events still NULL after cold resync *(OPEN — likely tied to KI-MOD-01)*
- After cold resync: 11 complete + 1 withdraw events have NULL
  `project_db_id`. All cluster around slots 170M–173M, on
  KI-MOD-01-affected projects where modify events introduced milestones
  with non-original IDs (MS-N gaps).
- **Hypothesis:** `find_project_from_inputs`
  (`event_processor.rs`) uses `collect_milestone_id_hints` to
  disambiguate when chain trace finds multiple candidate projects (see
  KI-OC-03). When the event's milestone IDs (e.g., `MS-15`) appear in
  modify-created milestone rows on more than one project, the hint
  scoring is ambiguous and trace returns `None`.
- **Status:** investigation pending. Slight regression from the 4 NULLs
  seen pre-cold-resync; the wipe also wiped any partially-seeded chain
  state. Resolution probably involves tightening
  `collect_milestone_id_hints` to use both milestone_id AND
  milestone_order, or falling back to the input UTXO's
  `project_db_id` directly when ambiguous.

**Repro query**

```sql
SELECT event_type,
       COUNT(*) FILTER (WHERE project_db_id IS NULL) AS null_project,
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

#### KI-FIN-04 — per-project balance under-counts the raw PSSC total *(OPEN — TODO)*
- **Pattern:** `v_projects_summary.current_balance_lovelace` joins live
  `yaci_store.address_utxo` against `treasury.utxo_history` so that each
  unspent PSSC UTXO is attributed to the specific project that funded it.
  Unspent PSSC UTXOs that `utxo_history` has *not* attributed to a project
  (chain-trace gaps — primarily KI-EVT-01-residual / KI-MOD-01-affected
  projects whose modify-chain we didn't fully trace) are excluded from
  every project's per-project balance.
- **Currently affected:** sum of per-project balances ≈ 80.65M ADA vs the
  raw on-chain PSSC total of 88.34M ADA — gap of ~7.7M ADA sits at the
  shared PSSC address but isn't claimed by any project row.
- **Why this is OK at the treasury level:** `v_financial_summary
  .project_balance_lovelace` and `/api/v1/statistics.financials
  .current_balance_lovelace` deliberately use the *raw* PSSC SUM (not the
  attributed sum), so the treasury-level total reports the on-chain truth.
  The under-count only surfaces if a consumer sums per-project balances.
- **Proposed fix (deferred):** resolve the underlying chain-trace gaps via
  KI-MOD-01 (modify-tx datum re-parse) and KI-EVT-01-residual
  (`collect_milestone_id_hints` disambiguation tightening). Once chain
  trace covers every PSSC UTXO, attributed sum should match raw PSSC sum.

**Repro query**

```sql
SELECT
  (SELECT SUM(au.lovelace_amount)
     FROM yaci_store.address_utxo au
     JOIN treasury.vendor_contracts vc ON vc.address = au.owner_addr
     WHERE NOT EXISTS (SELECT 1 FROM yaci_store.tx_input ti
                       WHERE ti.tx_hash=au.tx_hash AND ti.output_index=au.output_index))
  / 1e6 AS raw_pssc_ada,
  (SELECT SUM(current_balance_lovelace) FROM treasury.v_projects_summary)
  / 1e6 AS attributed_pssc_ada;
```

---

### A.5 `treasury.utxo_history` (formerly `treasury.utxos`)

#### KI-UTX-01 — `treasury.utxo_history` table + Postgres trigger *(IMPLEMENTED — verified by 2026-05-02 cold resync)*
- **Implementation:** `install_utxo_history_triggers`
  (`api/src/services/sync.rs`) creates two triggers at API startup:
  - `capture_address_utxo` AFTER INSERT/UPDATE on `yaci_store.address_utxo`
    copies every `addr1x*` row into `treasury.utxo_history`.
  - `mark_utxo_spent` AFTER INSERT on `yaci_store.tx_input` flags the
    corresponding `treasury.utxo_history` row as spent.
- **Outcome:** complete UTXO history at script addresses is preserved
  regardless of YACI Store's pruning window. Resolves KI-VND-04,
  KI-EVT-01, KI-CR-01 — all confirmed by the 2026-05-02 cold resync.

#### KI-UTX-02 — `project_db_id` IS NULL on non-script UTXOs (by design)
- **Why:** `pre_fetch_utxos` inserts every output of every TOM-event tx
  without `project_db_id`. The chain-trace seed (set later by
  `process_fund` and `find_project_from_inputs`) only fills it
  for outputs at the script address. Non-script change/fee outputs
  remain NULL by design — they aren't part of the chain.
- **Currently affected:** 786 / 1235 rows. Not anomalous — expected.

**Repro query**

```sql
SELECT
  CASE
    WHEN project_db_id IS NOT NULL AND address IS NOT NULL THEN 'fully_tracked'
    WHEN project_db_id IS NULL AND address IS NOT NULL THEN 'address_only'
    WHEN address IS NULL THEN 'sparse'
    ELSE 'other'
  END AS state,
  COUNT(*) AS count
FROM treasury.utxo_history GROUP BY 1 ORDER BY 2 DESC;
```

**Current breakdown:** `address_only=786`, `fully_tracked=449`.

#### KI-UTX-03 — NULL `lovelace_amount` rows *(RESOLVED — verified by 2026-05-02 cold resync)*
- Previously 5 / 1222 rows had NULL `lovelace_amount` (outputs whose
  `yaci_store.address_utxo` row was already pruned by the time we looked
  it up). With the historical-UTXO trigger capturing rows on insert, no
  such gaps remain.
- **Currently affected:** 0 / 1235.

**Repro query**

```sql
SELECT tx_hash, output_index, address
FROM treasury.utxo_history WHERE lovelace_amount IS NULL;
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
- **Effect on complete events:** of 189 complete events, 108 use `m-N` keys
  and 81 use `MS-N` keys. After the disambiguation hint to
  `find_project_from_inputs`, this no longer causes silent event
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
- **Currently affected:** complete 189/189, withdraw 129/129, pause 63/63,
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
- **Mitigation in code:** `find_project_from_inputs`
  (`event_processor.rs`) now scores candidate `project_db_id`s
  against `body.milestones` keys and prefers the one whose stored milestones
  match (`collect_milestone_id_hints`).
- **Currently affected:** observable indirectly via KI-EVT-02 = 0.

---

## Section C — Cold-replay UTXO-pruning limitation

### KI-CR-01 — Fresh local sync can't reconstruct fully-pruned chains *(RESOLVED — verified by 2026-05-02 cold resync)*
- **Resolved by:** the historical-UTXO trigger (KI-UTX-01). Going forward,
  every UTXO YACI Store inserts is captured in `treasury.utxo_history` before
  it can be pruned, so continuous-operation chain trace works fully.
- **Verified:** the 2026-05-02 cold resync was run with
  `install_utxo_history_triggers` arming the triggers before YACI Store
  began ingesting. KI-VND-04 and KI-EVT-01 both improved to ~zero in this
  run, confirming the recovery procedure works end-to-end. (KI-VND-01 and
  the datum-derived part of KI-MIL-01 remain — those are parser issues,
  not pruning issues.)
- **Caveat:** the trigger only protects UTXOs *from the moment it was
  installed*. If `yaci_store.address_utxo` had already been pruned before
  the trigger was armed, historical fund-output datums may still be
  missing from `treasury.utxo_history`. To recover those, wipe both
  schemas and re-sync from `STORE_CARDANO_SYNC_START_SLOT`:
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

### KI-SY-02 — `last_slot` can advance past failed events on connection reset *(RESOLVED — Phase 1 + periodic full sync shipped)*

**Resolution:** the contiguous-success watermark in `sync.rs` (Phase 1
in the proposed fix below) is now in place. Additionally, a separate
`tokio::spawn` task runs `sync_all_events` every 10 minutes as a safety
net — any event that wedges the incremental loop is recovered by the
next full re-sync via the idempotent `ON CONFLICT DO UPDATE` chain.
Phase 2 (a `treasury.failed_events` table + per-event retry interval)
was de-scoped in favour of the simpler periodic full sync, which proved
sufficient when applied to the KI-VND-01 cascade.

The original analysis is preserved below for context.



- **Symptom observed:** during a postgres restart mid-batch
  (2026-04-28), 5 events failed to insert. Continuous-sync logged
  `Sync error: error communicating with database` then advanced `last_slot`
  past those events on the next successful batch, so they were never
  retried.
- **Why** (confirmed by reading `api/src/services/sync.rs:67–146`):
  ```rust
  let mut last_processed_slot = last_slot;
  for row in rows {
      if let Err(e) = processor.process_event(&row).await {
          tracing::error!("Failed to process event {}: {}", row.tx_hash, e);
          continue;                                    // <-- skip
      }
      last_processed_slot = row.slot.unwrap_or(last_processed_slot);  // <-- bumps past skipped
  }
  ```
  A success at row `i+1` bumps the watermark past a skipped row `i`. The
  watermark is then persisted to `treasury.sync_status` (line ~139),
  making the skipped event unrecoverable until the API restarts and
  `sync_all_events` reprocesses from slot 0.
- **Why retries are safe**: all inserts use `ON CONFLICT (tx_hash) DO
  UPDATE` (`event_processor.rs:1057–1084`, `:327`, `:432–453`,
  `:1227–1233`), and child-table updates COALESCE to preserve existing
  values. Re-applying any event is idempotent.

##### Proposed fix — Phase 1 (small, ship first)

Replace the watermark loop with a contiguous-success tracker:

```rust
let mut watermark = last_slot;
let mut hole_seen = false;
for row in rows {
    match processor.process_event(&row).await {
        Err(e) => {
            tracing::error!(
                "Failed to process event {} at slot {:?}: {:#}",
                row.tx_hash, row.slot, e
            );
            hole_seen = true;
        }
        Ok(()) => {
            if !hole_seen {
                watermark = row.slot.unwrap_or(watermark);
            }
        }
    }
}
```

- **Cost:** if an event fails *permanently* (e.g., schema mismatch), the
  loop wedges at that slot until an operator intervenes. That's the
  point — silent loss is worse than visible stall, and the WARN log
  surfaces it.

##### Proposed fix — Phase 2 (durable, follow-up)

Add a `treasury.failed_events` table and a periodic auto-retry:

```sql
CREATE TABLE treasury.failed_events (
    tx_hash       VARCHAR(64) PRIMARY KEY,
    slot          BIGINT,
    event_type    TEXT,
    error         TEXT NOT NULL,
    retry_count   INT NOT NULL DEFAULT 0,
    first_seen    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_attempt  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_failed_events_retry
    ON treasury.failed_events (retry_count, last_attempt);
```

- On the `Err` path in the loop, upsert (`ON CONFLICT (tx_hash) DO
  UPDATE SET retry_count = retry_count + 1, last_attempt = NOW(),
  error = EXCLUDED.error`).
- Spawn a tokio interval (e.g. every 10 min) that selects from
  `treasury.failed_events` and re-runs `process_event` for each — same
  idempotent path. Delete the row on success.
- Operator visibility: `SELECT * FROM treasury.failed_events ORDER BY
  retry_count DESC` shows the backlog. Optional: expose a count on the
  `/api/v1/statistics` endpoint.

##### Operational note when this lands

`treasury.sync_status.last_slot` semantics shift from "last successful
row" to "last contiguous success". Operationally invisible to consumers
of `/api/v1/status`, but worth a one-line release note.

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

| ID | Area | Status | Blocked on |
|---|---|---|---|
| KI-VND-01 | NULL `vendor_payment_key_hash` on `UTXO-*` projects | **resolved** (6-bug cascade, 10/42 → 0/42 post cold resync) | — |
| KI-VND-02 | `vendor_name` deprecated | **resolved** (column dropped) | — |
| KI-VND-03 | `contract_url` deprecated | **resolved** (column dropped) | — |
| KI-VND-04 | `contract_address` NULL on cold replay | **resolved** (verified, 0/42) | — |
| KI-VND-05 | 2 corrupted utxo_history datums from prior bug #4 | **resolved** (cold resync + bug #6 merged-source query) | — |
| KI-MIL-01 (`label`) | NULL `label` for `UTXO-*` | **resolved** (description fallback, 0/364) | — |
| KI-MIL-01 (`amount`/`time_limit`) | NULL datum-derived fields for `UTXO-*` | **largely resolved** (136/386 → 16/364) | KI-MOD-01 |
| KI-MIL-01 (`acceptance_criteria`) | NULL for `UTXO-*` | **not a bug** — metadata genuinely lacks the field | — |
| KI-EVT-01 | NULL `project_db_id` on chain-trace failure | **resolved** (verified, 12/413 residual upstream) | — |
| KI-EVT-01-residual | 12 events still NULL on KI-MOD-01-affected projects | **open** — likely milestone-id-hint disambiguation issue | KI-MOD-01 |
| KI-MOD-01 | `modify` events don't update milestone amounts / time limits in API | **open** — TODO | — |
| KI-FIN-04 | per-project balance under-counts raw PSSC total (chain-trace gaps) | **open** — TODO | KI-MOD-01 |
| KI-EVT-03 | NULL `milestone_id` on treasury-level events | by design | — |
| KI-UTX-01 | historical-UTXO table + trigger | **implemented & verified** | — |
| KI-UTX-02 | `project_db_id` NULL on non-script UTXOs | by design | — |
| KI-UTX-03 | NULL `lovelace_amount` rows | **resolved** (verified, 0/1235) | — |
| KI-OC-01 | milestone-id naming drift (m-N vs MS-N) | **resolved at lookup time** | — |
| KI-OC-02 | empty `body.identifier` everywhere | on-chain limitation | — |
| KI-OC-03 | multi-input sibling-project txs | resolved (disambiguation hint) | — |
| KI-CR-01 | cold-replay limitation | **resolved** (verified by 2026-05-02 cold resync) | — |
| KI-SY-01 | idle `updated_at` doesn't bump | **resolved** | — |
| KI-SY-02 | `last_slot` advances past failed events | **resolved** (contiguous-success watermark + periodic full sync) | — |
| KI-API-01 | `destination` schema mismatch | **resolved** (JSONB; breaking API change) | — |
