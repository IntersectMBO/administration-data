# TOM Event Processing Reference

Comprehensive reference for how TOM (Treasury Oversight Metadata) events flow from on-chain metadata through the event processor into the treasury database schema.

**Spec source**: [SundaeSwap treasury-contracts spec.md](https://github.com/SundaeSwap-finance/treasury-contracts/blob/main/offchain/src/metadata/spec.md)

---

## 1. On-Chain Architecture (Corrected)

### Contract Structure

```
Treasury Contract (TRSC)
  - ONE unique script address per treasury instance
  - Holds the treasury reserve funds

Vendor Contract (PSSC)
  - ONE shared script address for ALL projects
  - Each fund tx creates UTXOs at this shared address
  - UTXOs belong to specific projects, distinguished by inline datum, NOT by address
```

**Critical insight**: The codebase historically assumed each project gets its own unique PSSC script address. In reality, **all projects share ONE vendor contract script address**. The relationship is:

```
                    ┌──────────────────────────────┐
                    │   Treasury Contract (TRSC)    │
                    │   unique script address        │
                    └──────────┬───────────────────┘
                               │ fund events
                               ▼
                    ┌──────────────────────────────┐
                    │ Shared Vendor Contract (PSSC) │
                    │ ONE script address for ALL     │
                    │ projects                       │
                    └──────────┬───────────────────┘
                               │
              ┌────────────────┼────────────────┐
              ▼                ▼                ▼
         ┌─────────┐    ┌─────────┐     ┌─────────┐
         │ UTXO A  │    │ UTXO B  │     │ UTXO C  │
         │Project 1│    │Project 2│     │Project 3│
         │(datum)  │    │(datum)  │     │(datum)  │
         └─────────┘    └─────────┘     └─────────┘
```

UTXOs at the shared address are distinguished by their **inline datum** (containing milestone amounts, time limits, etc.) and by their **origin** (which fund transaction created them), not by the address they sit at.

### Implications for UTXO Tracking

- `find_vendor_contract_from_inputs()` traces inputs back through the UTXO chain — the correct and only approach for linking events to projects
- UTXO tracking relies exclusively on chain tracing by specific (tx_hash, output_index) pairs, not by address

---

## 2. TOM Metadata Format

### Top-Level Structure

All TOM metadata is submitted under CIP-100 metadata label **1694**:

```json
{
  "@context": "<spec-version-url>",
  "hashAlgorithm": "blake2b-256",
  "txAuthor": "<pubkeyhash of tx signer>",
  "instance": "<treasury instance identifier>",
  "body": {
    "event": "<event-type>",
    ...event-specific fields...
  }
}
```

- **`@context`**: URL pointing to the metadata specification version (varies by event type)
- **`hashAlgorithm`**: Always `"blake2b-256"`
- **`txAuthor`**: Public key hash; must appear in the transaction's required signers
- **`instance`**: Filters to the configured treasury (matches `TREASURY_INSTANCE` env var)
- **`body.event`**: Determines event type — the processor dispatches on this field

### Code path for extraction

```
event.body → JSON
  → body.get("body").get("event") → event_type string
  → body.get("instance") → instance string
  → match event_type → process_<type>()
```

### CIP-100 Text Chunking

Text fields may be either a plain string or an array of 64-character chunks that must be joined:

```json
"label": "Short name"

"description": [
  "This is a long description that has been split into 64-cha",
  "racter chunks per the CIP-100 standard for on-chain storag",
  "e."
]
```

The `extract_text` / `extract_text_from_value` helpers handle both formats, joining arrays with `""` (empty string, no separator).

---

## 3. Event Type Reference

### publish

**Purpose**: Creates a new treasury instance by describing the published scriptRegistry datum.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"publish"` |
| `label` | string | Human-readable name for the instance |
| `description` | string | Markdown-formatted description |
| `expiration` | number | POSIX timestamp for instance expiration |
| `payoutUpperbound` | number | Maximum payout amount |
| `vendorExpiration` | number | Expiration timestamp for vendor contracts |
| `seedUtxo` | object | `{transactionId, outputIndex}` |
| `permissions` | object | Map of action names → permission definitions |

#### Code Extraction (`process_publish`)
| Metadata Path | Extracted As | DB Column |
|---------------|-------------|-----------|
| `body.label` | `extract_text()` → name | `treasury_contracts.name` |
| `body.permissions` | raw JSON clone | `treasury_contracts.permissions` |

**Not extracted**: `description`, `expiration`, `payoutUpperbound`, `vendorExpiration`, `seedUtxo`

#### DB Writes
- **UPSERT** `treasury.treasury_contracts` (keyed on `contract_instance`)
- **INSERT** `treasury.events`

---

### initialize

**Purpose**: Documents the initialization of a treasury instance (stake address withdrawal).

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"initialize"` |
| `reason` | string | Justification (optional) |
| `outputs` | object | Map of output indices → `{identifier, label}` |

#### Code Extraction (`process_initialize`)
Minimal — only records the tx hash and block time.

#### DB Writes
- **UPSERT** `treasury.treasury_contracts` — sets `initialized_tx_hash`, `initialized_at`
- **INSERT** `treasury.events`

**Not extracted**: `reason`, `outputs`

---

### fund

**Purpose**: Records funds flowing from treasury into the vendor contract, creating a new project.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"fund"` |
| `identifier` | string | Unique project ID (e.g., `"EC-0008-25"`) |
| `otherIdentifiers` | array | Related project IDs |
| `label` | string | Project title (often includes vendor name by convention) |
| `description` | string | Markdown project description |
| `vendor` | object | `{label: "<vendor name>", details: {anchorUrl, anchorDataHash}}` |
| `contract` | object | `{anchorUrl: "<contract doc URL>", anchorDataHash}` |
| `milestones` | object | Map of milestone IDs → milestone objects |

**Spec milestone object** (keyed by ID in an object, e.g., `{"m-0": {...}}`):
| Field | Type | Description |
|-------|------|-------------|
| `identifier` | string | Milestone ID matching datum |
| `label` | string | Human-readable name |
| `description` | string | Markdown description |
| `acceptanceCriteria` | string | Completion criteria |
| `details` | object | Additional details (optional) |

#### Code Extraction (`process_fund`)
| Metadata Path | Extracted As | DB Column |
|---------------|-------------|-----------|
| `body.identifier` | string | `vendor_contracts.project_id` |
| `body.label` | `extract_text()` | `vendor_contracts.project_name` |
| `body.description` | `extract_text()` | `vendor_contracts.description` |
| _(not extracted)_ | `None` | `vendor_contracts.vendor_name` (always null — TOM spec has no `vendor.name` field) |
| `body.vendor.label` | `extract_text_from_value()` | `vendor_contracts.vendor_address` |
| `body.contract` | `extract_contract()` — handles both string and `{anchorUrl}` object | `vendor_contracts.contract_url` |
| `body.otherIdentifiers` | string array | `vendor_contracts.other_identifiers` |
| `body.milestones[].identifier` | string | `milestones.milestone_id` |
| `body.milestones[].label` | `extract_text_from_value()` | `milestones.label` |
| `body.milestones[].description` | `extract_text_from_value()` | `milestones.description` |
| `body.milestones[].acceptanceCriteria` | `extract_text_from_value()` | `milestones.acceptance_criteria` |
| `body.milestones[].amount` | i64 | `milestones.amount_lovelace` |

**Milestone format handling**: Milestones are accepted in both array format (`[{identifier: "m-0", ...}]`) and object format (`{"m-0": {...}}`). For arrays, the `identifier` field inside each element provides the milestone ID. For objects, the key is the milestone ID.

Additionally queries `yaci_store.address_utxo` for the fund tx to get:
- `contract_address` — first `addr1x%` output address
- `initial_amount_lovelace` — lovelace amount of that output

**Datum integration**: After UTXO recording, queries `inline_datum` from the fund tx output (`addr1x%` address). If available, parses the CBOR datum via `parse_vendor_contract_datum()` to:
- Store `vendor_payment_key_hash` on the vendor contract row
- Update each milestone's `amount_lovelace`, `time_limit`, and `paused` flag from the datum (overwriting metadata-provided amounts with authoritative on-chain values)
- Store raw CBOR hex on the UTXO tracking row (`inline_datum_cbor`)

#### DB Writes
- **UPSERT** `treasury.treasury_contracts` (ensure exists)
- **INSERT** `treasury.vendor_contracts` (ON CONFLICT by `project_id` updates name/description)
- **INSERT** `treasury.milestones` (one per milestone, ON CONFLICT DO NOTHING)
- **INSERT** `treasury.events`
- **INSERT** `treasury.utxos` (record output UTXOs for chain tracking, with `inline_datum_cbor` if available)
- **UPDATE** `treasury.vendor_contracts` — sets `vendor_payment_key_hash` (from datum, if available)
- **UPDATE** `treasury.milestones` — sets `amount_lovelace`, `time_limit`, `paused` per milestone (from datum, if available)

---

### complete

**Purpose**: Vendor provides evidence of milestone completion by spending the vendor contract UTXO without withdrawing funds.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"complete"` |
| `milestones` | object | Map of milestone IDs → `{description, evidence[]}` |

**Evidence array item**:
| Field | Type | Description |
|-------|------|-------------|
| `label` | string | Evidence description |
| `anchorUrl` | string | Evidence location |
| `anchorDataHash` | string | Document hash (optional) |

#### Code Extraction (`process_complete`)
| Metadata Path | Extracted As | DB Column |
|---------------|-------------|-----------|
| `body.identifier` | string (fallback) | Used to find vendor_contract_id |
| `body.milestones.<id>.description` | `extract_text_from_value()` | `milestones.complete_description` |
| `body.milestones.<id>.evidence` | raw JSON clone | `milestones.evidence` |
| `body.milestone` | string (legacy format) | Used to find milestone by ID |

**Project identification**: First tries `body.identifier` to look up vendor contract by project_id. Falls back to `find_vendor_contract_from_inputs()` (UTXO chain tracing).

**Milestone format handling**: Code handles milestones as an object keyed by milestone ID (`.as_object()`), which matches the spec. Also handles legacy single `body.milestone` field as a fallback.

#### DB Writes
- **UPDATE** `treasury.milestones` — sets `evidence_provided = TRUE`, `complete_tx_hash`, `complete_time`, `complete_description`, `evidence`
- **INSERT** `treasury.events` (one per milestone completed)

---

### disburse

**Purpose**: Treasury-level fund movement — moves funds from the treasury contract to an external destination (e.g., stablecoin minting). **Not milestone-related.**

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"disburse"` |
| `label` | string | Human-readable transaction title |
| `description` | string | Mechanical description of fund usage |
| `justification` | string | Markdown explaining committee remit |
| `destination` | object/array | `{label, details: {anchorUrl, anchorDataHash}}` |
| `estimatedReturn` | number | POSIX timestamp for expected fund return |

#### Code Extraction (`process_disburse`)
| Metadata Path | Extracted As | DB Column |
|---------------|-------------|-----------|
| `instance` (top-level) | string | Used to look up `treasury_id` directly |
| `body.destination` | `extract_text()` | `events.destination` |

**Not extracted**: `label`, `description`, `justification`, `estimatedReturn`

Disburse is a treasury-level operation. The code looks up `treasury_id` from `instance` and does **not** call `find_vendor_contract_from_inputs`. `vendor_contract_id` is always `None` for disburse events.

**Note**: `destination` extraction uses `extract_text()` which expects a string or string array, while the spec defines destination as an object with `label`/`details`. This means structured destination metadata may not be fully captured.

#### DB Writes
- **INSERT** `treasury.events` (with destination field, `vendor_contract_id = NULL`)

---

### withdraw

**Purpose**: Vendor claims matured milestone funds from the vendor contract.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"withdraw"` |
| `milestones` | object | Map of milestone IDs → `{comment}` |

#### Code Extraction (`process_withdraw`)
| Metadata Path | Extracted As | DB Column |
|---------------|-------------|-----------|
| `body.identifier` | string | Used to find vendor_contract_id |
| `body.milestones` | object keyed by milestone ID | Iterates over all milestone IDs |
| `body.milestone` | string (legacy fallback) | Used to find milestone by ID if `milestones` absent |

**Milestone format handling**: Code first checks for `body.milestones` (plural) as an object keyed by milestone ID (spec format, handles multiple milestones per withdraw). Falls back to `body.milestone` (singular string) for legacy single-milestone format.

Additionally queries `yaci_store.address_utxo` for the withdraw tx to calculate `withdraw_amount` (sum of non-script outputs via `owner_addr NOT LIKE 'addr1x%'`).

**Not extracted**: `milestones.<id>.comment`

#### DB Writes
- **UPDATE** `treasury.milestones` — sets `withdrawn = TRUE`, `withdraw_tx_hash`, `withdraw_time`, `withdraw_amount`
- **INSERT** `treasury.events`

---

### pause

**Purpose**: Oversight committee prevents milestone fund withdrawal pending resolution.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"pause"` |
| `milestones` | object | Map of milestone IDs → `{reason, resolution}` |

#### Code Extraction (`process_pause`)
| Metadata Path | Extracted As | DB Column |
|---------------|-------------|-----------|
| `body.identifier` | string | Used to find vendor_contract_id |
| `body.reason` | `extract_text()` | `events.reason` |

**Per-milestone pause via datum**: After identifying the vendor contract, calls `update_milestone_pause_from_datum()` which parses the output datum of the pause transaction. Each milestone in the datum has a `Constr(0|1, [])` pause flag (0=active, 1=paused), and the code updates the `paused` boolean on each milestone row accordingly.

**Contract-level status derivation**: After updating per-milestone flags, the code derives contract status: `paused` if ALL milestones are paused, `active` if no milestones are paused. Mixed state leaves the contract status unchanged.

**Not extracted**: per-milestone `reason`, `resolution` from metadata

#### DB Writes
- **UPDATE** `treasury.milestones` — sets `paused` flag per milestone (from datum)
- **UPDATE** `treasury.vendor_contracts` — sets `status` to `'paused'` or `'active'` (derived from per-milestone state)
- **INSERT** `treasury.events` (with reason)

---

### resume

**Purpose**: Oversight committee resumes previously paused milestone payments.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"resume"` |
| `milestones` | object | Map of milestone IDs → `{reason}` |

#### Code Extraction (`process_resume`)
| Metadata Path | Extracted As | DB Column |
|---------------|-------------|-----------|
| `body.identifier` | string | Used to find vendor_contract_id |

**Per-milestone resume via datum**: Same mechanism as pause. After identifying the vendor contract, calls `update_milestone_pause_from_datum()` which parses the output datum to read each milestone's pause flag and updates the `paused` boolean per milestone row.

**Contract-level status derivation**: Same as pause — `active` if no milestones paused, `paused` if all milestones paused.

**Not extracted**: per-milestone `reason` from metadata

#### DB Writes
- **UPDATE** `treasury.milestones` — sets `paused` flag per milestone (from datum)
- **UPDATE** `treasury.vendor_contracts` — sets `status` to `'paused'` or `'active'` (derived from per-milestone state)
- **INSERT** `treasury.events`

---

### modify

**Purpose**: Vendor and committee agree to alter payout amounts or milestone terms.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"modify"` |
| `identifier` | string | Project ID being modified |
| `otherIdentifiers` | array | Related project IDs |
| `label` | string | Updated project title |
| `description` | string | Updated project description |
| `reason` | string | Markdown justification |
| `vendor` | object | Updated vendor info (same format as fund) |
| `contract` | object | Updated contract info (same format as fund) |
| `milestones` | object/array | Updated milestone definitions |

#### Code Extraction (`process_modify`)
| Metadata Path | Extracted As | DB Column |
|---------------|-------------|-----------|
| `body.identifier` | string | Used to find vendor_contract_id |
| `body.label` | `extract_text()` | `vendor_contracts.project_name` (COALESCE update) |
| `body.description` | `extract_text()` | `vendor_contracts.description` (COALESCE update) |
| `body.vendor.label` | `extract_text_from_value()` | `vendor_contracts.vendor_address` (COALESCE update) |
| `body.contract` | `extract_contract()` | `vendor_contracts.contract_url` (COALESCE update) |
| `body.reason` | `extract_text()` | `events.reason` |
| `body.milestones` | array or object of milestones | Archives old, inserts new |

**Naming fields update**: Before processing milestones, the code extracts `label`, `description`, `vendor.label`, and `contract` and updates the vendor contract row using COALESCE (only overwrites if the new value is non-null).

**Milestone format handling**: Same as fund — milestones are accepted in both array format (`[{identifier: "m-0", ...}]`) and object format (`{"m-0": {...}}`).

Milestone field extraction is identical to fund (identifier, label, description, acceptanceCriteria, amount).

#### DB Writes
- **UPDATE** `treasury.vendor_contracts` — COALESCE update of `project_name`, `description`, `vendor_address`, `contract_url`
- **UPDATE** `treasury.milestones` — sets `archived = TRUE`, `archived_by_tx_hash`, `archived_at` for all active milestones
- **INSERT** `treasury.milestones` — new milestone rows
- **UPDATE** `treasury.milestones` — sets `superseded_by` FK linking old → new rows with matching milestone_id
- **INSERT** `treasury.events` (with reason)

---

### cancel

**Purpose**: Special case of modify where project is completely cancelled and refunded.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"cancel"` |
| `reason` | string | Markdown explanation for cancellation |

#### Code Extraction (`process_cancel`)
| Metadata Path | Extracted As | DB Column |
|---------------|-------------|-----------|
| `body.identifier` | string | Used to find vendor_contract_id |
| `body.reason` | `extract_text()` | `events.reason` |

#### DB Writes
- **UPDATE** `treasury.vendor_contracts` — sets `status = 'cancelled'`
- **INSERT** `treasury.events` (with reason)

---

### sweep

**Purpose**: Returns surplus funds from treasury or vendor contracts back to the Cardano treasury.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"sweep"` |
| `comment` | string | Markdown explanation (optional; metadata may be omitted entirely) |

#### Code Extraction (`process_sweep`)
Minimal — only looks up treasury_id from instance.

**Not extracted**: `comment`

#### DB Writes
- **INSERT** `treasury.events`

Note: Code also matches `"sweeptreasury"` and `"sweepvendor"` as aliases.

---

### reorganize

**Purpose**: Documents fund splitting, merging, or rebalancing operations.

#### Spec Fields
| Field | Type | Description |
|-------|------|-------------|
| `event` | string | `"reorganize"` |
| `reason` | string | Justification (optional) |
| `outputs` | object | Map of output indices → `{identifier, label}` |

#### Code Extraction (`process_reorganize`)
Minimal — only looks up treasury_id from instance.

**Not extracted**: `reason`, `outputs`

#### DB Writes
- **INSERT** `treasury.events`

---

## 4. Field Extraction Details

### Text Extraction Helpers

```rust
fn extract_text(obj: &Value, field: &str) -> Option<String>
fn extract_text_from_value(value: Option<&Value>) -> Option<String>
```

Both handle two formats:
- **String**: returned as-is
- **Array of strings**: joined with `""` (empty string — no separator)

**Known issue**: The join with empty string means `["Hello ", "World"]` → `"Hello World"` (correct) but `["Hello", "World"]` → `"HelloWorld"` (missing space). CIP-100 chunks at fixed byte boundaries, so this typically works for continuous text but could produce incorrect results at chunk boundaries if the original text doesn't align.

### Vendor Name vs Label

The TOM spec defines the `vendor` object as:
```json
{
  "vendor": {
    "label": "Vendor Company Name",
    "details": {
      "anchorUrl": "https://...",
      "anchorDataHash": "..."
    }
  }
}
```

The code sets `vendor_name = None` explicitly — TOM spec has no `vendor.name` field, so `vendor_contracts.vendor_name` is always null. `vendor.label` is extracted via `extract_text_from_value()` into `vendor_contracts.vendor_address`.

In practice, vendor identity comes from the top-level `body.label` which by convention includes the vendor name (e.g., `"Tastenkunst GmbH - Eternl Maintenance"`). The `vendor.label` field in real metadata contains the vendor's payment address (a Cardano address), not their display name.

### Contract URL Extraction

The `extract_contract()` helper handles both metadata formats:

- **String**: `"contract": "https://..."` — returned directly
- **Object**: `"contract": {"anchorUrl": "https://...", "anchorDataHash": "..."}` — extracts `anchorUrl`

This covers both spec-conformant metadata (object format) and simplified metadata (plain string).

### Milestone Format: Object vs Array

| Context | Spec Format | Code Handles |
|---------|------------|-------------|
| fund | Object keyed by ID: `{"m-0": {...}}` | Both array `[{identifier: "m-0", ...}]` and object `{"m-0": {...}}` |
| complete | Object keyed by ID: `{"m-0": {...}}` | Object keyed by ID (correct) |
| modify | Same as fund | Both array and object (same as fund handler) |
| withdraw | Object keyed by ID: `{"m-0": {...}}` | Object `milestones` (plural, keyed by ID) + singular string `milestone` fallback |
| pause | Object keyed by ID: `{"m-0": {...}}` | Per-milestone via inline datum parsing (not from metadata milestones field) |
| resume | Object keyed by ID: `{"m-0": {...}}` | Per-milestone via inline datum parsing (not from metadata milestones field) |

Real on-chain metadata uses both arrays and objects for fund/modify events. The code handles both formats.

---

## 5. UTXO Chain Tracking

### How It Works

When a `fund` event is processed, the code records all output UTXOs from that transaction in `treasury.utxos` with the `vendor_contract_id`. Subsequent events (complete, withdraw, etc.) spend those UTXOs, so the processor can trace backwards to find which project an event belongs to.

### `find_vendor_contract_from_inputs()`

```
1. Get inputs to this tx: SELECT tx_hash, output_index FROM yaci_store.tx_input
                          WHERE spent_tx_hash = $1
2. For each input, look up: SELECT vendor_contract_id FROM treasury.utxos
                             WHERE tx_hash = $1 AND output_index = $2
3. If found: mark old UTXO as spent, record new output UTXOs with same vendor_contract_id
4. Return first matching vendor_contract_id
```

This correctly traces the UTXO chain regardless of address, because it tracks by specific (tx_hash, output_index) pairs.

When recording new output UTXOs (step 3), the code also stores `inline_datum_cbor` if the output has an inline datum in `yaci_store.address_utxo`. This datum is used later by pause/resume processing.

---

## 5a. Datum Integration

### CBOR Datum Parser (`parsers/datum.rs`)

The datum parser decodes inline Plutus datums from CBOR hex into structured data. It uses the `pallas` library for CBOR decoding.

### Datum Structure

```text
Constr(0, [
  Constr(0, [ByteString(vendor_payment_key_hash)]),
  Array([
    Constr(0, [BigInt(time_limit), Map(value), Constr(0|1, [])]),  // per milestone
    ...
  ])
])
```

- **vendor_payment_key_hash**: hex-encoded byte string identifying the vendor's payment key
- **Per-milestone fields**:
  - `BigInt(time_limit)` — POSIXTime in milliseconds, the milestone's expiration
  - `Map(value)` — Plutus Value map, structured as `{"": {"": lovelace_amount}}` (ADA policy ID is empty bytestring)
  - `Constr(0|1, [])` — pause flag: constructor 0 (tag 121) = active, constructor 1 (tag 122) = paused

### When Datums Are Parsed

| Context | Function | What happens |
|---------|----------|-------------|
| `fund` event | `parse_vendor_contract_datum()` | Populates `vendor_payment_key_hash`, per-milestone `amount_lovelace`, `time_limit`, `paused` |
| `pause` event | `update_milestone_pause_from_datum()` | Updates per-milestone `paused` flags, derives contract status |
| `resume` event | `update_milestone_pause_from_datum()` | Updates per-milestone `paused` flags, derives contract status |
| UTXO chain tracking | `find_vendor_contract_from_inputs()` | Stores `inline_datum_cbor` on new UTXO rows for later use |

### Fields Extracted

| Datum Field | DB Column | Table |
|-------------|-----------|-------|
| `vendor_payment_key_hash` | `vendor_payment_key_hash` | `vendor_contracts` |
| Per-milestone `time_limit` | `time_limit` | `milestones` |
| Per-milestone lovelace from Value map | `amount_lovelace` | `milestones` |
| Per-milestone `Constr(0\|1)` | `paused` | `milestones` |

### Prerequisite

Requires `store.script.enabled=true` in YACI Store configuration (`indexer/application.properties`) so that `inline_datum` is populated on `address_utxo` rows. If disabled, datum parsing is skipped gracefully.

---

## 6. Known Bugs & Limitations (Resolved)

All 11 bugs have been fixed. This section documents the original issues and their resolutions.

### Critical (Fixed)

**1. ~~`sync_address_utxos()` misassigns UTXOs~~** — FIXED: Deleted `sync_utxos()` and `sync_address_utxos()`. UTXO tracking now relies exclusively on `find_vendor_contract_from_inputs()` chain tracing.

**2. ~~`vendor.name` always null~~** — FIXED: Code now sets `vendor_name = None` explicitly since TOM spec has no `vendor.name` field. `vendor.label` correctly maps to `vendor_address`.

### High (Fixed)

**3. ~~Disburse events incorrectly linked to vendor contracts~~** — FIXED: `process_disburse` now takes `instance` parameter and looks up `treasury_id` directly. No longer calls `find_vendor_contract_from_inputs`. `vendor_contract_id` is always `None` for disburse events.

**4. Multiple UTXO inputs → first match wins** — Acceptable: A transaction spending vendor contract UTXOs belongs to one project. First-match is the correct behavior.

**5. ~~Pause/resume are contract-level, spec says milestone-level~~** — FIXED: Added `paused` boolean flag on milestones. `process_pause`/`process_resume` now parse the output datum to determine per-milestone pause state via `update_milestone_pause_from_datum()`. Contract-level status is derived: paused if ALL milestones paused, active if none paused.

**6. ~~Modify doesn't update naming fields~~** — FIXED: `process_modify` now extracts and updates `project_name`, `description`, `vendor_address`, and `contract_url` using COALESCE before processing milestones.

### Medium (Fixed)

**7. ~~Array text concat has no separator~~** — Correct behavior: CIP-100 splits text at fixed 64-byte boundaries, so `join("")` correctly reconstructs the original text. Added explanatory comment.

**8. ~~Fund milestones as array vs spec object~~** — FIXED: Both `process_fund` and `process_modify` now handle milestones as either an array `[{identifier: "m-0", ...}]` or an object `{"m-0": {...}}`.

**9. ~~No slot-level ordering within blocks~~** — FIXED: Added `m.tx_hash ASC` as secondary sort in both `sync_all_events` and `sync_new_events` queries.

**10. ~~`contract` field extraction assumes string~~** — FIXED: Added `extract_contract()` helper that handles both `contract: "url"` (string) and `contract: {anchorUrl: "url"}` (object) formats.

**11. ~~Withdraw handles single milestone only~~** — FIXED: `process_withdraw` now checks for `milestones` object (plural, keyed by ID) first, falling back to singular `milestone` field for legacy format.

---

## 7. Debugging Queries

### Compare raw metadata vs stored values for a project

```sql
-- Get raw metadata for a project's fund event
SELECT e.tx_hash, e.metadata
FROM treasury.events e
JOIN treasury.vendor_contracts vc ON vc.id = e.vendor_contract_id
WHERE vc.project_id = 'EC-0008-25' AND e.event_type = 'fund';

-- Compare with stored values
SELECT project_id, project_name, vendor_name, vendor_address, contract_url, description
FROM treasury.vendor_contracts
WHERE project_id = 'EC-0008-25';
```

### Find projects with null vendor_name

```sql
SELECT project_id, project_name, vendor_name, vendor_address
FROM treasury.vendor_contracts
WHERE vendor_name IS NULL
ORDER BY project_id;
```

### Check for duplicate contract_addresses across projects

```sql
-- All projects sharing the same contract address (expected: all share one)
SELECT contract_address, COUNT(*) as project_count,
       array_agg(project_id ORDER BY project_id) as projects
FROM treasury.vendor_contracts
WHERE contract_address IS NOT NULL
GROUP BY contract_address
HAVING COUNT(*) > 1;
```

### Verify UTXO assignment correctness

```sql
-- Check if UTXOs at the shared address are spread across projects or concentrated on one
SELECT u.vendor_contract_id, vc.project_id, COUNT(*) as utxo_count,
       SUM(u.lovelace_amount) as total_lovelace
FROM treasury.utxos u
JOIN treasury.vendor_contracts vc ON vc.id = u.vendor_contract_id
WHERE NOT u.spent
GROUP BY u.vendor_contract_id, vc.project_id
ORDER BY utxo_count DESC;
```

### Check UTXO chain integrity for a project

```sql
-- Follow the UTXO chain for a specific project
WITH RECURSIVE utxo_chain AS (
    SELECT u.tx_hash, u.output_index, u.spent, u.spent_tx_hash, u.vendor_contract_id, 1 as depth
    FROM treasury.utxos u
    JOIN treasury.vendor_contracts vc ON vc.id = u.vendor_contract_id
    WHERE vc.project_id = 'EC-0008-25'
      AND u.tx_hash = vc.fund_tx_hash

    UNION ALL

    SELECT u.tx_hash, u.output_index, u.spent, u.spent_tx_hash, u.vendor_contract_id, uc.depth + 1
    FROM treasury.utxos u
    JOIN utxo_chain uc ON u.tx_hash = uc.spent_tx_hash
    WHERE uc.spent = true AND uc.depth < 20
)
SELECT * FROM utxo_chain ORDER BY depth;
```

### Compare events across projects

```sql
-- All events with project context, ordered by time
SELECT e.event_type, e.block_time, e.tx_hash,
       vc.project_id, vc.project_name
FROM treasury.events e
LEFT JOIN treasury.vendor_contracts vc ON vc.id = e.vendor_contract_id
ORDER BY e.block_time DESC
LIMIT 50;
```

### Inspect metadata for a specific event type

```sql
-- View raw metadata for all complete events
SELECT e.tx_hash, e.block_time,
       vc.project_id,
       e.metadata->'body'->'milestones' as milestones_meta
FROM treasury.events e
LEFT JOIN treasury.vendor_contracts vc ON vc.id = e.vendor_contract_id
WHERE e.event_type = 'complete';
```
