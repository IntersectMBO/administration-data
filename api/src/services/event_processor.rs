//! TOM Event Processor
//!
//! Processes TOM (Treasury Oversight Metadata) events and updates the
//! normalized treasury schema tables.

use sqlx::PgPool;
use serde_json::Value;

use super::sync::RawTomEvent;

/// Event processor for TOM metadata
pub struct EventProcessor {
    pool: PgPool,
}

impl EventProcessor {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Sync all events from the beginning (for initial sync)
    pub async fn sync_all_events(&self) -> anyhow::Result<()> {
        // Get all TOM events ordered by slot
        let rows = sqlx::query_as::<_, RawTomEvent>(
            r#"
            SELECT
                m.tx_hash,
                m.slot,
                m.body::jsonb as body,
                b.number as block_number,
                b.block_time
            FROM yaci_store.transaction_metadata m
            JOIN yaci_store.block b ON b.slot = m.slot
            WHERE m.label = '1694'
            ORDER BY m.slot ASC, m.tx_hash ASC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        tracing::info!("Processing {} total TOM events", rows.len());

        // Pre-fetch UTXOs in batches before processing to guard against pruning
        let tx_hashes: Vec<String> = rows.iter().map(|r| r.tx_hash.clone()).collect();
        for chunk in tx_hashes.chunks(100) {
            if let Err(e) = self.pre_fetch_utxos(chunk).await {
                tracing::warn!("UTXO pre-fetch failed (non-fatal): {}", e);
            }
        }

        let mut processed = 0;
        for row in &rows {
            if let Err(e) = self.process_event(row).await {
                tracing::warn!("Failed to process event {}: {}", row.tx_hash, e);
                continue;
            }
            processed += 1;
        }

        tracing::info!("Processed {} events successfully", processed);

        // Update sync status with last event
        if let Some(last) = rows.last() {
            sqlx::query(
                r#"
                UPDATE treasury.sync_status
                SET last_slot = $1, last_block = $2, last_tx_hash = $3, updated_at = NOW()
                WHERE sync_type = 'events'
                "#
            )
            .bind(last.slot.unwrap_or(0))
            .bind(last.block_number.unwrap_or(0))
            .bind(&last.tx_hash)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Process a single TOM event
    pub async fn process_event(&self, event: &RawTomEvent) -> anyhow::Result<()> {
        let body = match &event.body {
            Some(b) => b,
            None => return Ok(()), // No body, skip
        };

        let event_type = body.get("body")
            .and_then(|b| b.get("event"))
            .and_then(|e| e.as_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        let instance = body.get("instance")
            .and_then(|i| i.as_str())
            .unwrap_or("");

        match event_type.as_str() {
            "publish" => self.process_publish(event, body, instance).await?,
            "initialize" => self.process_initialize(event, body, instance).await?,
            "fund" => self.process_fund(event, body, instance).await?,
            "complete" => self.process_complete(event, body).await?,
            "disburse" => self.process_disburse(event, body, instance).await?,
            "withdraw" => self.process_withdraw(event, body).await?,
            "pause" => self.process_pause(event, body).await?,
            "resume" => self.process_resume(event, body).await?,
            "modify" => self.process_modify(event, body).await?,
            "cancel" => self.process_cancel(event, body).await?,
            "sweep" | "sweeptreasury" | "sweepvendor" => self.process_sweep(event, body, instance).await?,
            "reorganize" => self.process_reorganize(event, body, instance).await?,
            _ => {
                tracing::debug!("Unknown event type: {}", event_type);
            }
        }

        Ok(())
    }

    /// Process a publish event - create treasury contract
    async fn process_publish(&self, event: &RawTomEvent, body: &Value, instance: &str) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);
        let permissions = event_body.get("permissions").cloned();

        // Upsert treasury contract
        let treasury_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO treasury.treasury_contracts (contract_instance, publish_tx_hash, publish_time, permissions)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (contract_instance) DO UPDATE
                SET publish_tx_hash = COALESCE(treasury.treasury_contracts.publish_tx_hash, EXCLUDED.publish_tx_hash),
                    publish_time = COALESCE(treasury.treasury_contracts.publish_time, EXCLUDED.publish_time),
                    permissions = COALESCE(EXCLUDED.permissions, treasury.treasury_contracts.permissions)
            RETURNING id
            "#
        )
        .bind(instance)
        .bind(&event.tx_hash)
        .bind(event.block_time)
        .bind(&permissions)
        .fetch_one(&self.pool)
        .await?;

        // Insert event record
        self.insert_event_full(event, "publish", Some(treasury_id), None, None, None, &None, &None, body).await?;

        Ok(())
    }

    /// Process an initialize event - update treasury contract
    async fn process_initialize(&self, event: &RawTomEvent, body: &Value, instance: &str) -> anyhow::Result<()> {
        // Upsert treasury contract
        let treasury_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO treasury.treasury_contracts (contract_instance, initialized_tx_hash, initialized_at)
            VALUES ($1, $2, $3)
            ON CONFLICT (contract_instance) DO UPDATE
                SET initialized_tx_hash = COALESCE(treasury.treasury_contracts.initialized_tx_hash, EXCLUDED.initialized_tx_hash),
                    initialized_at = COALESCE(treasury.treasury_contracts.initialized_at, EXCLUDED.initialized_at)
            RETURNING id
            "#
        )
        .bind(instance)
        .bind(&event.tx_hash)
        .bind(event.block_time)
        .fetch_one(&self.pool)
        .await?;

        // Extract treasury contract address from tx outputs (with fallback for pruned UTXOs)
        let (contract_address, _, _) = self.get_script_utxo_for_tx(&event.tx_hash).await?;

        if let Some(ref addr) = contract_address {
            let stake_cred = crate::parsers::address::extract_stake_credential(addr);
            sqlx::query("UPDATE treasury.treasury_contracts SET contract_address = COALESCE(contract_address, $1), stake_credential = COALESCE(stake_credential, $2) WHERE id = $3")
                .bind(addr)
                .bind(&stake_cred)
                .bind(treasury_id)
                .execute(&self.pool)
                .await?;
        }

        let event_body = body.get("body").unwrap_or(body);
        let reason = extract_text(event_body, "reason");

        self.insert_event_full(event, "initialize", Some(treasury_id), None, None, None, &reason, &None, body).await?;

        Ok(())
    }

    /// Process a fund event - create vendor contract and milestones
    async fn process_fund(&self, event: &RawTomEvent, body: &Value, instance: &str) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);

        let raw_identifier = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .unwrap_or("");

        if raw_identifier.is_empty() {
            return Ok(());
        }

        // Split space-separated identifiers: first becomes project_id, rest merge into other_identifiers
        let id_parts: Vec<&str> = raw_identifier.split_whitespace().collect();
        let project_id = id_parts[0];
        let extra_ids: Vec<String> = id_parts[1..].iter().map(|s| s.to_string()).collect();

        let project_name = extract_text(event_body, "label");
        let description = extract_text(event_body, "description");
        let vendor_address = event_body.get("vendor")
            .and_then(|v| extract_text_from_value(v.get("label")));
        let mut other_identifiers: Vec<String> = event_body.get("otherIdentifiers")
            .and_then(|o| o.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        other_identifiers.extend(extra_ids);
        let other_identifiers = if other_identifiers.is_empty() { None } else { Some(other_identifiers) };

        // Get contract address, initial amount, and inline datum from fund tx output
        // (with fallback to treasury.utxo_history for pruned UTXOs)
        let (contract_address, initial_amount, fund_inline_datum) = self.get_script_utxo_for_tx(&event.tx_hash).await?;

        // Get or create treasury contract
        let treasury_id: Option<i32> = if !instance.is_empty() {
            sqlx::query_scalar(
                r#"
                INSERT INTO treasury.treasury_contracts (contract_instance)
                VALUES ($1)
                ON CONFLICT (contract_instance) DO UPDATE SET contract_instance = EXCLUDED.contract_instance
                RETURNING id
                "#
            )
            .bind(instance)
            .fetch_optional(&self.pool)
            .await?
        } else {
            None
        };

        // Fallback: populate treasury contract_address if still null
        // The treasury address is the addr1x input that differs from the vendor contract output
        if let Some(tid) = treasury_id {
            if let Some(ref vc_addr) = contract_address {
                let treasury_addr: Option<String> = sqlx::query_scalar(
                    r#"
                    SELECT DISTINCT au.owner_addr
                    FROM yaci_store.tx_input ti
                    JOIN yaci_store.address_utxo au ON au.tx_hash = ti.tx_hash AND au.output_index = ti.output_index
                    WHERE ti.spent_tx_hash = $1 AND au.owner_addr LIKE 'addr1x%' AND au.owner_addr != $2
                    LIMIT 1
                    "#
                )
                .bind(&event.tx_hash)
                .bind(vc_addr)
                .fetch_optional(&self.pool)
                .await?;

                // Fallback: use treasury.utxo_history for pruned input UTXOs
                let treasury_addr = match treasury_addr {
                    Some(addr) => Some(addr),
                    None => sqlx::query_scalar::<_, Option<String>>(
                        r#"
                        SELECT DISTINCT u.address
                        FROM yaci_store.transaction t
                        CROSS JOIN LATERAL jsonb_array_elements(t.inputs::jsonb) AS inp
                        JOIN treasury.utxo_history u
                            ON u.tx_hash = inp->>'tx_hash'
                            AND u.output_index = (inp->>'output_index')::smallint
                        WHERE t.tx_hash = $1 AND u.address LIKE 'addr1x%' AND u.address != $2
                        LIMIT 1
                        "#
                    )
                    .bind(&event.tx_hash)
                    .bind(vc_addr)
                    .fetch_optional(&self.pool)
                    .await?
                    .flatten(),
                };

                if let Some(ref addr) = treasury_addr {
                    let stake_cred = crate::parsers::address::extract_stake_credential(addr);
                    sqlx::query("UPDATE treasury.treasury_contracts SET contract_address = COALESCE(contract_address, $1), stake_credential = COALESCE(stake_credential, $2) WHERE id = $3")
                        .bind(addr)
                        .bind(&stake_cred)
                        .bind(tid)
                        .execute(&self.pool)
                        .await?;
                }
            }
        }

        // Upsert the singleton vendor contract row (one per shared PSSC address)
        // so /api/v1/vendor-contract has something to return. Reject any address
        // that matches the treasury contract — `get_script_utxo_for_tx` may return
        // the treasury change output if the vendor output happens to be later in
        // the tx. Filtering here prevents the treasury address from leaking into
        // the vendor_contracts singleton.
        if let Some(ref vc_addr) = contract_address {
            let stake_cred = crate::parsers::address::extract_stake_credential(vc_addr);
            sqlx::query(
                r#"
                INSERT INTO treasury.vendor_contracts (treasury_id, address, stake_credential)
                SELECT $1, $2, $3
                WHERE NOT EXISTS (
                    SELECT 1 FROM treasury.treasury_contracts
                    WHERE contract_address = $2
                )
                ON CONFLICT (address) DO UPDATE
                    SET treasury_id = COALESCE(EXCLUDED.treasury_id, treasury.vendor_contracts.treasury_id),
                        stake_credential = COALESCE(EXCLUDED.stake_credential, treasury.vendor_contracts.stake_credential)
                "#,
            )
            .bind(treasury_id)
            .bind(vc_addr)
            .bind(&stake_cred)
            .execute(&self.pool)
            .await?;
        }

        // Insert project
        let project_db_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO treasury.projects (
                treasury_id, project_id, other_identifiers, project_name, description,
                vendor_address, contract_address,
                fund_tx_hash, fund_slot, fund_block_time, initial_amount_lovelace, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'active')
            ON CONFLICT (project_id) DO UPDATE
                SET project_name = COALESCE(EXCLUDED.project_name, treasury.projects.project_name),
                    description = COALESCE(EXCLUDED.description, treasury.projects.description)
            RETURNING id
            "#
        )
        .bind(treasury_id)
        .bind(project_id)
        .bind(&other_identifiers)
        .bind(&project_name)
        .bind(&description)
        .bind(&vendor_address)
        .bind(&contract_address)
        .bind(&event.tx_hash)
        .bind(event.slot)
        .bind(event.block_time)
        .bind(initial_amount)
        .fetch_one(&self.pool)
        .await?;

        // Process milestones — handle both array format and object format (keyed by ID)
        let milestones_list: Vec<(String, &Value)> = if let Some(milestones_val) = event_body.get("milestones") {
            if let Some(arr) = milestones_val.as_array() {
                arr.iter().enumerate().map(|(idx, m)| {
                    let id = m.get("identifier")
                        .and_then(|i| i.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("m-{}", idx));
                    (id, m)
                }).collect()
            } else if let Some(obj) = milestones_val.as_object() {
                obj.iter().map(|(k, v)| (k.clone(), v)).collect()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        for (idx, (milestone_id_str, milestone)) in milestones_list.iter().enumerate() {
                let milestone_id = milestone_id_str.as_str();
                let acceptance_criteria = extract_text_from_value(Some(milestone.get("acceptanceCriteria").unwrap_or(&Value::Null)));
                let (label, description) = extract_milestone_label_description(
                    extract_text_from_value(Some(milestone.get("label").unwrap_or(&Value::Null))),
                    extract_text_from_value(Some(milestone.get("description").unwrap_or(&Value::Null))),
                    &acceptance_criteria,
                );
                let amount = milestone.get("amount")
                    .and_then(|a| a.as_i64());

                sqlx::query(
                    r#"
                    INSERT INTO treasury.milestones (
                        project_db_id, milestone_id, milestone_order, label,
                        description, acceptance_criteria, amount_lovelace
                    )
                    VALUES ($1, $2, $3, $4, $5, $6, $7)
                    ON CONFLICT (project_db_id, milestone_id) WHERE NOT archived DO NOTHING
                    "#
                )
                .bind(project_db_id)
                .bind(milestone_id)
                .bind((idx + 1) as i32)
                .bind(&label)
                .bind(&description)
                .bind(&acceptance_criteria)
                .bind(amount)
                .execute(&self.pool)
                .await?;
        }

        self.insert_event_full(event, "fund", treasury_id, Some(project_db_id), None, initial_amount, &None, &None, body).await?;

        // Record the output UTXOs from this fund transaction for future lookups
        // Get all outputs from the transaction table
        let outputs: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT outputs::jsonb FROM yaci_store.transaction WHERE tx_hash = $1"
        )
        .bind(&event.tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(serde_json::Value::Array(output_arr)) = outputs {
            for output in output_arr {
                if let (Some(tx_hash), Some(output_index)) = (
                    output.get("tx_hash").and_then(|h| h.as_str()),
                    output.get("output_index").and_then(|i| i.as_i64())
                ) {
                    // Look up address and amount (with fallback for pruned UTXOs)
                    let (looked_up_address, lovelace_amount, _) = self.lookup_utxo(tx_hash, output_index as i16).await?;

                    // For known addresses, only track script (addr1x) outputs and skip change.
                    // For pruned outputs (address unknown), assume the fund event's contract_address
                    // — the chain trace only matches by (tx_hash, output_index, project_db_id),
                    // so over-seeding a non-script output is harmless and lets cold replay link
                    // milestone events whose input UTXOs have already been pruned.
                    let address = match looked_up_address {
                        Some(addr) if addr.starts_with("addr1x") => Some(addr),
                        Some(_) => continue,
                        None => contract_address.clone(),
                    };

                    let address_type = Some("vendor_contract");

                    // Record this UTXO with the project_db_id for future event lookups
                    sqlx::query(
                        r#"
                        INSERT INTO treasury.utxo_history (tx_hash, output_index, project_db_id, slot, block_number, address, address_type, lovelace_amount, spent)
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, false)
                        ON CONFLICT (tx_hash, output_index) DO UPDATE
                            SET project_db_id = COALESCE(EXCLUDED.project_db_id, treasury.utxo_history.project_db_id),
                                address = COALESCE(EXCLUDED.address, treasury.utxo_history.address),
                                address_type = COALESCE(EXCLUDED.address_type, treasury.utxo_history.address_type),
                                lovelace_amount = COALESCE(EXCLUDED.lovelace_amount, treasury.utxo_history.lovelace_amount),
                                block_number = COALESCE(EXCLUDED.block_number, treasury.utxo_history.block_number)
                        "#
                    )
                    .bind(tx_hash)
                    .bind(output_index as i16)
                    .bind(project_db_id)
                    .bind(event.slot)
                    .bind(event.block_number)
                    .bind(&address)
                    .bind(address_type)
                    .bind(lovelace_amount)
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        // Parse inline datum for milestone amounts, time_limits, and vendor_payment_key_hash
        // Uses the datum already fetched by get_script_utxo_for_tx (with pruning fallback)
        if let Some(datum_hex) = fund_inline_datum {
            match crate::parsers::datum::parse_project_datum(&datum_hex) {
                Ok(parsed) => {
                    // Store vendor_payment_key_hash
                    sqlx::query(
                        "UPDATE treasury.projects SET vendor_payment_key_hash = $1 WHERE id = $2"
                    )
                    .bind(&parsed.vendor_payment_key_hash)
                    .bind(project_db_id)
                    .execute(&self.pool)
                    .await?;

                    // Update milestones with datum data (amount, time_limit, paused).
                    // Skip withdrawn milestones — they are consumed on-chain, so the
                    // datum only contains entries for non-withdrawn milestones.
                    let milestone_rows: Vec<(i32,)> = sqlx::query_as(
                        "SELECT id FROM treasury.milestones WHERE project_db_id = $1 AND NOT archived AND NOT withdrawn ORDER BY milestone_order"
                    )
                    .bind(project_db_id)
                    .fetch_all(&self.pool)
                    .await?;

                    for (datum_idx, (db_id,)) in milestone_rows.iter().enumerate() {
                        if let Some(ms_datum) = parsed.milestones.get(datum_idx) {
                            sqlx::query(
                                r#"
                                UPDATE treasury.milestones
                                SET amount_lovelace = $1, time_limit = $2, paused = $3
                                WHERE id = $4
                                "#
                            )
                            .bind(ms_datum.amount_lovelace)
                            .bind(ms_datum.time_limit)
                            .bind(ms_datum.paused)
                            .bind(db_id)
                            .execute(&self.pool)
                            .await?;
                        }
                    }

                    // Store raw CBOR on the UTXO tracking row
                    sqlx::query(
                        "UPDATE treasury.utxo_history SET inline_datum_cbor = $1 WHERE tx_hash = $2 AND project_db_id = $3"
                    )
                    .bind(&datum_hex)
                    .bind(&event.tx_hash)
                    .bind(project_db_id)
                    .execute(&self.pool)
                    .await?;
                }
                Err(e) => {
                    tracing::debug!("Could not parse fund datum for {}: {}", event.tx_hash, e);
                }
            }
        }

        Ok(())
    }

    /// Process a complete event - update milestone status
    async fn process_complete(&self, event: &RawTomEvent, body: &Value) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);

        let project_id_from_meta = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty());

        // Hints used to disambiguate when chain tracing finds multiple candidate
        // vendor contracts (e.g. the tx pulls fee inputs from a sibling project).
        let milestone_hints = collect_milestone_id_hints(event_body);

        let project_db_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "SELECT id FROM treasury.projects WHERE project_id = $1"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            self.find_project_from_inputs(&event.tx_hash, &milestone_hints).await?
        };

        if project_db_id.is_none() {
            tracing::warn!("Could not find vendor contract for complete event {}", event.tx_hash);
        }

        let mut matched_milestone_id: Option<i32> = None;

        if let Some(vc_id) = project_db_id {
            if let Some(obj) = event_body.get("milestones").and_then(|m| m.as_object()) {
                for (milestone_id, milestone_data) in obj {
                    let description = extract_text_from_value(Some(milestone_data.get("description").unwrap_or(&Value::Null)));
                    let evidence = milestone_data.get("evidence").cloned();
                    let order_hint = canonical_milestone_order(milestone_id);

                    let db_milestone_id: Option<i32> = sqlx::query_scalar(
                        r#"
                        UPDATE treasury.milestones
                        SET evidence_provided = TRUE,
                            complete_tx_hash = $1,
                            complete_time = $2,
                            complete_description = $3,
                            evidence = $4
                        WHERE project_db_id = $5
                          AND NOT archived
                          AND (milestone_id = $6 OR milestone_order = $7)
                        RETURNING id
                        "#
                    )
                    .bind(&event.tx_hash)
                    .bind(event.block_time)
                    .bind(&description)
                    .bind(&evidence)
                    .bind(vc_id)
                    .bind(milestone_id)
                    .bind(order_hint)
                    .fetch_optional(&self.pool)
                    .await?;

                    if matched_milestone_id.is_none() {
                        matched_milestone_id = db_milestone_id;
                    }
                }
            }

            if let Some(milestone_id) = event_body.get("milestone").and_then(|m| m.as_str()) {
                let order_hint = canonical_milestone_order(milestone_id);
                let db_milestone_id: Option<i32> = sqlx::query_scalar(
                    r#"
                    UPDATE treasury.milestones
                    SET evidence_provided = TRUE,
                        complete_tx_hash = $1,
                        complete_time = $2
                    WHERE project_db_id = $3
                      AND NOT archived
                      AND (milestone_id = $4 OR milestone_order = $5)
                    RETURNING id
                    "#
                )
                .bind(&event.tx_hash)
                .bind(event.block_time)
                .bind(vc_id)
                .bind(milestone_id)
                .bind(order_hint)
                .fetch_optional(&self.pool)
                .await?;

                if matched_milestone_id.is_none() {
                    matched_milestone_id = db_milestone_id;
                }
            }
        }

        self.insert_event_full(event, "complete", None, project_db_id, matched_milestone_id, None, &None, &None, body).await?;

        Ok(())
    }

    /// Process a disburse event - treasury-level fund movement (does not touch milestones)
    async fn process_disburse(&self, event: &RawTomEvent, body: &Value, instance: &str) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);
        // Per TOM spec, `destination` is an object `{label, details}`. Preserve the
        // full object in JSONB so neither sub-field is lost (KI-API-01).
        let destination = event_body.get("destination").cloned();

        // Disburse is a treasury-level operation — look up treasury_id, not project_db_id
        let treasury_id: Option<i32> = if !instance.is_empty() {
            sqlx::query_scalar(
                "SELECT id FROM treasury.treasury_contracts WHERE contract_instance = $1"
            )
            .bind(instance)
            .fetch_optional(&self.pool)
            .await?
        } else {
            None
        };

        self.insert_event_full(event, "disburse", treasury_id, None, None, None, &None, &destination, body).await?;

        Ok(())
    }

    /// Process a withdraw event - vendor claims matured milestone funds
    async fn process_withdraw(&self, event: &RawTomEvent, body: &Value) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);

        let project_id_from_meta = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty());

        let milestone_hints = collect_milestone_id_hints(event_body);

        let project_db_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "SELECT id FROM treasury.projects WHERE project_id = $1"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            self.find_project_from_inputs(&event.tx_hash, &milestone_hints).await?
        };

        if project_db_id.is_none() {
            tracing::warn!("Could not find vendor contract for withdraw event {}", event.tx_hash);
        }

        // Withdraw amount comes from tx outputs (non-script addresses) and is independent of vc lookup.
        let withdraw_amount: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(SUM(lovelace_amount)::bigint, 0) FROM yaci_store.address_utxo WHERE tx_hash = $1 AND owner_addr NOT LIKE 'addr1x%'"
        )
        .bind(&event.tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        let withdraw_amount = match withdraw_amount {
            Some(amt) if amt > 0 => Some(amt),
            _ => {
                let fallback: Option<i64> = sqlx::query_scalar(
                    "SELECT COALESCE(SUM(lovelace_amount)::bigint, 0) FROM treasury.utxo_history WHERE tx_hash = $1 AND address NOT LIKE 'addr1x%'"
                )
                .bind(&event.tx_hash)
                .fetch_optional(&self.pool)
                .await?;
                match fallback {
                    Some(a) if a > 0 => Some(a),
                    _ => withdraw_amount,
                }
            }
        };

        let mut matched_milestone_id: Option<i32> = None;

        if let Some(vc_id) = project_db_id {
            if let Some(obj) = event_body.get("milestones").and_then(|m| m.as_object()) {
                for (milestone_id, _milestone_data) in obj {
                    let order_hint = canonical_milestone_order(milestone_id);
                    let db_milestone_id: Option<i32> = sqlx::query_scalar(
                        r#"
                        UPDATE treasury.milestones
                        SET withdrawn = TRUE,
                            withdraw_tx_hash = $1,
                            withdraw_time = $2,
                            withdraw_amount = $3
                        WHERE project_db_id = $4
                          AND NOT archived
                          AND (milestone_id = $5 OR milestone_order = $6)
                        RETURNING id
                        "#
                    )
                    .bind(&event.tx_hash)
                    .bind(event.block_time)
                    .bind(withdraw_amount)
                    .bind(vc_id)
                    .bind(milestone_id)
                    .bind(order_hint)
                    .fetch_optional(&self.pool)
                    .await?;

                    if matched_milestone_id.is_none() {
                        matched_milestone_id = db_milestone_id;
                    }
                }
            }

            if let Some(milestone_id) = event_body.get("milestone").and_then(|m| m.as_str()) {
                let order_hint = canonical_milestone_order(milestone_id);
                let db_milestone_id: Option<i32> = sqlx::query_scalar(
                    r#"
                    UPDATE treasury.milestones
                    SET withdrawn = TRUE,
                        withdraw_tx_hash = $1,
                        withdraw_time = $2,
                        withdraw_amount = $3
                    WHERE project_db_id = $4
                      AND NOT archived
                      AND (milestone_id = $5 OR milestone_order = $6)
                    RETURNING id
                    "#
                )
                .bind(&event.tx_hash)
                .bind(event.block_time)
                .bind(withdraw_amount)
                .bind(vc_id)
                .bind(milestone_id)
                .bind(order_hint)
                .fetch_optional(&self.pool)
                .await?;

                if matched_milestone_id.is_none() {
                    matched_milestone_id = db_milestone_id;
                }
            }
        }

        self.insert_event_full(event, "withdraw", None, project_db_id, matched_milestone_id, withdraw_amount, &None, &None, body).await?;

        Ok(())
    }

    /// Process a pause event - set vendor contract status
    async fn process_pause(&self, event: &RawTomEvent, body: &Value) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);

        let project_id_from_meta = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty());
        let reason = extract_text(event_body, "reason");

        // Get vendor contract ID - either from metadata or by tracing tx chain
        let project_db_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "UPDATE treasury.projects SET status = 'paused' WHERE project_id = $1 RETURNING id"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            // Find via tx chain first, then update
            if let Some(vc_id) = self.find_project_from_inputs(&event.tx_hash, &[]).await? {
                sqlx::query("UPDATE treasury.projects SET status = 'paused' WHERE id = $1")
                    .bind(vc_id)
                    .execute(&self.pool)
                    .await?;
                Some(vc_id)
            } else {
                None
            }
        };

        if let Some(vc_id) = project_db_id {
            // Also update per-milestone pause flags from output datum if available
            self.update_milestone_pause_from_datum(&event.tx_hash, vc_id).await?;

            self.insert_event_full(event, "pause", None, Some(vc_id), None, None, &reason, &None, body).await?;
        } else {
            tracing::warn!("Could not find vendor contract for pause event {}", event.tx_hash);
            self.insert_event_full(event, "pause", None, None, None, None, &reason, &None, body).await?;
        }

        Ok(())
    }

    /// Process a resume event - set vendor contract status
    async fn process_resume(&self, event: &RawTomEvent, body: &Value) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);

        let project_id_from_meta = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty());

        // Get vendor contract ID - either from metadata or by tracing tx chain
        let project_db_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "UPDATE treasury.projects SET status = 'active' WHERE project_id = $1 RETURNING id"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            if let Some(vc_id) = self.find_project_from_inputs(&event.tx_hash, &[]).await? {
                sqlx::query("UPDATE treasury.projects SET status = 'active' WHERE id = $1")
                    .bind(vc_id)
                    .execute(&self.pool)
                    .await?;
                Some(vc_id)
            } else {
                None
            }
        };

        if let Some(vc_id) = project_db_id {
            // Also update per-milestone pause flags from output datum if available
            self.update_milestone_pause_from_datum(&event.tx_hash, vc_id).await?;

            self.insert_event_full(event, "resume", None, Some(vc_id), None, None, &None, &None, body).await?;
        } else {
            tracing::warn!("Could not find vendor contract for resume event {}", event.tx_hash);
            self.insert_event_full(event, "resume", None, None, None, None, &None, &None, body).await?;
        }

        Ok(())
    }

    /// Process a modify event - update vendor contract, archive and replace milestones
    async fn process_modify(&self, event: &RawTomEvent, body: &Value) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);

        let project_id_from_meta = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty());
        let reason = extract_text(event_body, "reason");

        // Get vendor contract ID - either from metadata or by tracing tx chain
        let project_db_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "SELECT id FROM treasury.projects WHERE project_id = $1"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            self.find_project_from_inputs(&event.tx_hash, &[]).await?
        };

        if let Some(vc_id) = project_db_id {
            // Update naming fields if present in modify metadata
            let project_name = extract_text(event_body, "label");
            let description = extract_text(event_body, "description");
            let vendor_address = event_body.get("vendor")
                .and_then(|v| extract_text_from_value(v.get("label")));

            sqlx::query(
                r#"
                UPDATE treasury.projects
                SET project_name = COALESCE($1, project_name),
                    description = COALESCE($2, description),
                    vendor_address = COALESCE($3, vendor_address)
                WHERE id = $4
                "#
            )
            .bind(&project_name)
            .bind(&description)
            .bind(&vendor_address)
            .bind(vc_id)
            .execute(&self.pool)
            .await?;

            // If milestones are present in the modify metadata, archive existing and insert new
            let milestones_list: Vec<(String, &Value)> = if let Some(milestones_val) = event_body.get("milestones") {
                if let Some(arr) = milestones_val.as_array() {
                    arr.iter().enumerate().map(|(idx, m)| {
                        let id = m.get("identifier")
                            .and_then(|i| i.as_str())
                            .unwrap_or(&format!("m-{}", idx))
                            .to_string();
                        (id, m)
                    }).collect()
                } else if let Some(obj) = milestones_val.as_object() {
                    obj.iter().map(|(k, v)| (k.clone(), v)).collect()
                } else {
                    vec![]
                }
            } else {
                vec![]
            };

            if !milestones_list.is_empty() {
                // Archive all active milestones for this vendor contract
                sqlx::query(
                    r#"
                    UPDATE treasury.milestones
                    SET archived = TRUE, archived_by_tx_hash = $1, archived_at = $2
                    WHERE project_db_id = $3 AND NOT archived
                    "#
                )
                .bind(&event.tx_hash)
                .bind(event.block_time)
                .bind(vc_id)
                .execute(&self.pool)
                .await?;

                // Insert new milestone rows
                for (idx, (milestone_id_str, milestone)) in milestones_list.iter().enumerate() {
                    let milestone_id = milestone_id_str.as_str();
                    let acceptance_criteria = extract_text_from_value(Some(milestone.get("acceptanceCriteria").unwrap_or(&Value::Null)));
                    let (label, description) = extract_milestone_label_description(
                        extract_text_from_value(Some(milestone.get("label").unwrap_or(&Value::Null))),
                        extract_text_from_value(Some(milestone.get("description").unwrap_or(&Value::Null))),
                        &acceptance_criteria,
                    );
                    let amount = milestone.get("amount")
                        .and_then(|a| a.as_i64());

                    let new_id: i32 = sqlx::query_scalar(
                        r#"
                        INSERT INTO treasury.milestones (
                            project_db_id, milestone_id, milestone_order, label,
                            description, acceptance_criteria, amount_lovelace
                        )
                        VALUES ($1, $2, $3, $4, $5, $6, $7)
                        RETURNING id
                        "#
                    )
                    .bind(vc_id)
                    .bind(milestone_id)
                    .bind((idx + 1) as i32)
                    .bind(&label)
                    .bind(&description)
                    .bind(&acceptance_criteria)
                    .bind(amount)
                    .fetch_one(&self.pool)
                    .await?;

                    // Update superseded_by on the archived row that matches this milestone_id
                    sqlx::query(
                        r#"
                        UPDATE treasury.milestones
                        SET superseded_by = $1
                        WHERE project_db_id = $2 AND milestone_id = $3 AND archived AND superseded_by IS NULL
                        "#
                    )
                    .bind(new_id)
                    .bind(vc_id)
                    .bind(milestone_id)
                    .execute(&self.pool)
                    .await?;
                }
            }

            self.insert_event_full(event, "modify", None, Some(vc_id), None, None, &reason, &None, body).await?;
        } else {
            tracing::warn!("Could not find vendor contract for modify event {}", event.tx_hash);
        }

        Ok(())
    }

    /// Process a cancel event - set vendor contract status
    async fn process_cancel(&self, event: &RawTomEvent, body: &Value) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);

        let project_id_from_meta = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty());
        let reason = extract_text(event_body, "reason");

        // Get vendor contract ID - either from metadata or by tracing tx chain
        let project_db_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "UPDATE treasury.projects SET status = 'cancelled' WHERE project_id = $1 RETURNING id"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            if let Some(vc_id) = self.find_project_from_inputs(&event.tx_hash, &[]).await? {
                sqlx::query("UPDATE treasury.projects SET status = 'cancelled' WHERE id = $1")
                    .bind(vc_id)
                    .execute(&self.pool)
                    .await?;
                Some(vc_id)
            } else {
                None
            }
        };

        if let Some(vc_id) = project_db_id {
            self.insert_event_full(event, "cancel", None, Some(vc_id), None, None, &reason, &None, body).await?;
        } else {
            tracing::warn!("Could not find vendor contract for cancel event {}", event.tx_hash);
        }

        Ok(())
    }

    /// Process a sweep event
    async fn process_sweep(&self, event: &RawTomEvent, body: &Value, instance: &str) -> anyhow::Result<()> {
        let treasury_id: Option<i32> = sqlx::query_scalar(
            "SELECT id FROM treasury.treasury_contracts WHERE contract_instance = $1"
        )
        .bind(instance)
        .fetch_optional(&self.pool)
        .await?;

        self.insert_event_full(event, "sweep", treasury_id, None, None, None, &None, &None, body).await?;

        Ok(())
    }

    /// Process a reorganize event
    async fn process_reorganize(&self, event: &RawTomEvent, body: &Value, instance: &str) -> anyhow::Result<()> {
        let treasury_id: Option<i32> = sqlx::query_scalar(
            "SELECT id FROM treasury.treasury_contracts WHERE contract_instance = $1"
        )
        .bind(instance)
        .fetch_optional(&self.pool)
        .await?;

        self.insert_event_full(event, "reorganize", treasury_id, None, None, None, &None, &None, body).await?;

        Ok(())
    }

    /// Insert an event record with all optional fields
    async fn insert_event_full(
        &self,
        event: &RawTomEvent,
        event_type: &str,
        treasury_id: Option<i32>,
        project_db_id: Option<i32>,
        milestone_id: Option<i32>,
        amount_lovelace: Option<i64>,
        reason: &Option<String>,
        destination: &Option<Value>,
        body: &Value,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO treasury.events (
                tx_hash, slot, block_number, block_time, event_type,
                treasury_id, project_db_id, milestone_id,
                amount_lovelace, reason, destination, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            ON CONFLICT (tx_hash) DO UPDATE SET
                amount_lovelace = COALESCE(EXCLUDED.amount_lovelace, treasury.events.amount_lovelace),
                reason = COALESCE(EXCLUDED.reason, treasury.events.reason),
                destination = COALESCE(EXCLUDED.destination, treasury.events.destination)
            "#
        )
        .bind(&event.tx_hash)
        .bind(event.slot)
        .bind(event.block_number)
        .bind(event.block_time)
        .bind(event_type)
        .bind(treasury_id)
        .bind(project_db_id)
        .bind(milestone_id)
        .bind(amount_lovelace)
        .bind(reason)
        .bind(destination)
        .bind(body)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Find project_db_id by looking up input UTXOs in our treasury.utxo_history tracking table.
    /// When a fund event is processed, its output UTXOs are recorded with the project_db_id.
    /// Subsequent events (complete/withdraw/etc) spend those UTXOs, so we can find the project
    /// by looking at which tracked UTXOs are being spent as inputs.
    async fn find_project_from_inputs(
        &self,
        tx_hash: &str,
        milestone_id_hints: &[String],
    ) -> anyhow::Result<Option<i32>> {
        // Get the inputs to this transaction from the transaction table's inputs JSONB.
        // We use this instead of yaci_store.tx_input because tx_input is pruned
        // (only retains ~44K recent slots), while transaction.inputs is permanent.
        let inputs_json: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT inputs::jsonb FROM yaci_store.transaction WHERE tx_hash = $1"
        )
        .bind(tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        let inputs: Vec<(String, i16)> = match inputs_json {
            Some(serde_json::Value::Array(arr)) => arr.iter().filter_map(|elem| {
                let tx = elem.get("tx_hash")?.as_str()?.to_string();
                let idx = elem.get("output_index")?.as_i64()? as i16;
                Some((tx, idx))
            }).collect(),
            _ => vec![],
        };

        // Collect every input that maps to a tracked vendor contract. A single tx can
        // include inputs from multiple project chains (e.g. fee/collateral from another
        // contract), so we don't want to commit to the first match blindly.
        let mut candidates: Vec<(String, i16, i32)> = Vec::new();
        for (input_tx_hash, input_output_index) in &inputs {
            let project_db_id: Option<i32> = sqlx::query_scalar(
                r#"
                SELECT project_db_id
                FROM treasury.utxo_history
                WHERE tx_hash = $1 AND output_index = $2 AND project_db_id IS NOT NULL
                "#
            )
            .bind(input_tx_hash)
            .bind(input_output_index)
            .fetch_optional(&self.pool)
            .await?;

            if let Some(vc_id) = project_db_id {
                candidates.push((input_tx_hash.clone(), *input_output_index, vc_id));
            }
        }

        if candidates.is_empty() {
            tracing::warn!("No tracked UTXO found for tx {} inputs", tx_hash);
            return Ok(None);
        }

        // Disambiguate when multiple project chains feed this tx. Prefer the candidate
        // whose stored milestones match the hint keys carried by the event metadata
        // (the `body.milestones` keys for complete/withdraw). Falls back to the first
        // candidate if no hint matches.
        let chosen_idx = if candidates.len() > 1 && !milestone_id_hints.is_empty() {
            let mut best_idx = 0usize;
            let mut best_score: i64 = -1;
            for (i, (_, _, vc_id)) in candidates.iter().enumerate() {
                let score: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM treasury.milestones WHERE project_db_id = $1 AND milestone_id = ANY($2)"
                )
                .bind(vc_id)
                .bind(milestone_id_hints)
                .fetch_one(&self.pool)
                .await?;
                if score > best_score {
                    best_score = score;
                    best_idx = i;
                }
            }
            best_idx
        } else {
            0
        };

        let (input_tx_hash, input_output_index, vc_id) = candidates[chosen_idx].clone();

        // Get the input UTXO's script address as a fallback for pruned outputs
        let input_address: Option<String> = sqlx::query_scalar(
            "SELECT address FROM treasury.utxo_history WHERE tx_hash = $1 AND output_index = $2"
        )
        .bind(&input_tx_hash)
        .bind(input_output_index)
        .fetch_optional(&self.pool)
        .await?
        .flatten();

        // Mark this UTXO as spent and record the new outputs
        sqlx::query(
            r#"
            UPDATE treasury.utxo_history
            SET spent = true, spent_tx_hash = $1
            WHERE tx_hash = $2 AND output_index = $3
            "#
        )
        .bind(tx_hash)
        .bind(&input_tx_hash)
        .bind(input_output_index)
        .execute(&self.pool)
        .await?;

        // Record the outputs of this transaction with the same project_db_id
        let outputs: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT outputs::jsonb FROM yaci_store.transaction WHERE tx_hash = $1"
        )
        .bind(tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(serde_json::Value::Array(output_arr)) = outputs {
            for output in output_arr {
                if let (Some(out_tx_hash), Some(output_index)) = (
                    output.get("tx_hash").and_then(|h| h.as_str()),
                    output.get("output_index").and_then(|i| i.as_i64())
                ) {
                    let (address, lovelace_amount, out_datum) = {
                        let (addr, amt, datum) = self.lookup_utxo(out_tx_hash, output_index as i16).await?;
                        if addr.is_some() {
                            (addr, amt, datum)
                        } else {
                            (input_address.clone(), None, None)
                        }
                    };

                    if !address.as_ref().map_or(false, |a| a.starts_with("addr1x")) {
                        continue;
                    }

                    let address_type = Some("vendor_contract");

                    sqlx::query(
                        r#"
                        INSERT INTO treasury.utxo_history (tx_hash, output_index, project_db_id, address, address_type, lovelace_amount, inline_datum_cbor, spent)
                        VALUES ($1, $2, $3, $4, $5, $6, $7, false)
                        ON CONFLICT (tx_hash, output_index) DO UPDATE
                            SET project_db_id = EXCLUDED.project_db_id,
                                address = COALESCE(EXCLUDED.address, treasury.utxo_history.address),
                                address_type = COALESCE(EXCLUDED.address_type, treasury.utxo_history.address_type),
                                lovelace_amount = COALESCE(EXCLUDED.lovelace_amount, treasury.utxo_history.lovelace_amount),
                                inline_datum_cbor = COALESCE(EXCLUDED.inline_datum_cbor, treasury.utxo_history.inline_datum_cbor)
                        "#
                    )
                    .bind(out_tx_hash)
                    .bind(output_index as i16)
                    .bind(vc_id)
                    .bind(&address)
                    .bind(address_type)
                    .bind(lovelace_amount)
                    .bind(&out_datum)
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        Ok(Some(vc_id))
    }

    /// Pre-fetch UTXOs from yaci_store into treasury.utxo_history before they can be pruned.
    /// Called before processing a batch of events to capture UTXO data that YACI Store
    /// may prune (spent UTXOs are removed after ~2160 blocks / ~10 days).
    pub async fn pre_fetch_utxos(&self, tx_hashes: &[String]) -> anyhow::Result<()> {
        if tx_hashes.is_empty() {
            return Ok(());
        }

        // Pre-fetch output UTXOs for event transactions
        let output_result = sqlx::query(
            r#"
            INSERT INTO treasury.utxo_history (tx_hash, output_index, address, lovelace_amount, inline_datum_cbor)
            SELECT au.tx_hash, au.output_index, au.owner_addr, au.lovelace_amount, au.inline_datum
            FROM yaci_store.address_utxo au
            WHERE au.tx_hash = ANY($1)
            ON CONFLICT (tx_hash, output_index) DO UPDATE
                SET address = COALESCE(EXCLUDED.address, treasury.utxo_history.address),
                    lovelace_amount = COALESCE(EXCLUDED.lovelace_amount, treasury.utxo_history.lovelace_amount),
                    inline_datum_cbor = COALESCE(EXCLUDED.inline_datum_cbor, treasury.utxo_history.inline_datum_cbor)
            "#
        )
        .bind(tx_hashes)
        .execute(&self.pool)
        .await?;

        // Pre-fetch input-side UTXOs (outputs being spent by these transactions)
        let input_result = sqlx::query(
            r#"
            INSERT INTO treasury.utxo_history (tx_hash, output_index, address, lovelace_amount, inline_datum_cbor)
            SELECT au.tx_hash, au.output_index, au.owner_addr, au.lovelace_amount, au.inline_datum
            FROM yaci_store.transaction t
            CROSS JOIN LATERAL jsonb_array_elements(t.inputs::jsonb) AS inp
            JOIN yaci_store.address_utxo au
                ON au.tx_hash = inp->>'tx_hash'
                AND au.output_index = (inp->>'output_index')::smallint
            WHERE t.tx_hash = ANY($1)
            ON CONFLICT (tx_hash, output_index) DO UPDATE
                SET address = COALESCE(EXCLUDED.address, treasury.utxo_history.address),
                    lovelace_amount = COALESCE(EXCLUDED.lovelace_amount, treasury.utxo_history.lovelace_amount),
                    inline_datum_cbor = COALESCE(EXCLUDED.inline_datum_cbor, treasury.utxo_history.inline_datum_cbor)
            "#
        )
        .bind(tx_hashes)
        .execute(&self.pool)
        .await?;

        let total = output_result.rows_affected() + input_result.rows_affected();
        if total > 0 {
            tracing::debug!("Pre-fetched {} UTXOs ({} outputs + {} inputs) into treasury.utxo_history",
                total, output_result.rows_affected(), input_result.rows_affected());
        }

        Ok(())
    }

    /// Fetch script UTXO data (address, lovelace_amount, inline_datum) for a transaction.
    /// Tries yaci_store.address_utxo first, falls back to pre-fetched treasury.utxo_history.
    async fn get_script_utxo_for_tx(&self, tx_hash: &str) -> anyhow::Result<(Option<String>, Option<i64>, Option<String>)> {
        let result: Option<(String, Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT owner_addr, lovelace_amount, inline_datum FROM yaci_store.address_utxo WHERE tx_hash = $1 AND owner_addr LIKE 'addr1x%' LIMIT 1"
        )
        .bind(tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((addr, amt, datum)) = result {
            return Ok((Some(addr), amt, datum));
        }

        // Fallback to pre-fetched treasury.utxo_history
        let result: Option<(Option<String>, Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT address, lovelace_amount, inline_datum_cbor FROM treasury.utxo_history WHERE tx_hash = $1 AND address LIKE 'addr1x%' LIMIT 1"
        )
        .bind(tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((addr, amt, datum)) = result {
            if addr.is_some() {
                tracing::debug!("Used treasury.utxo_history fallback for tx {} (yaci_store UTXO pruned)", tx_hash);
            }
            return Ok((addr, amt, datum));
        }

        Ok((None, None, None))
    }

    /// Look up a specific UTXO by tx_hash + output_index.
    /// Tries yaci_store.address_utxo first, falls back to treasury.utxo_history.
    async fn lookup_utxo(&self, tx_hash: &str, output_index: i16) -> anyhow::Result<(Option<String>, Option<i64>, Option<String>)> {
        let result: Option<(String, Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT owner_addr, lovelace_amount, inline_datum FROM yaci_store.address_utxo WHERE tx_hash = $1 AND output_index = $2 LIMIT 1"
        )
        .bind(tx_hash)
        .bind(output_index)
        .fetch_optional(&self.pool)
        .await?;

        if let Some((addr, amt, datum)) = result {
            return Ok((Some(addr), amt, datum));
        }

        let result: Option<(Option<String>, Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT address, lovelace_amount, inline_datum_cbor FROM treasury.utxo_history WHERE tx_hash = $1 AND output_index = $2 LIMIT 1"
        )
        .bind(tx_hash)
        .bind(output_index)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.unwrap_or((None, None, None)))
    }

    /// Update per-milestone pause flags from the output datum of a transaction
    async fn update_milestone_pause_from_datum(&self, tx_hash: &str, project_db_id: i32) -> anyhow::Result<()> {
        // Query inline datum from the tx output at the vendor contract address
        let inline_datum: Option<String> = sqlx::query_scalar(
            "SELECT inline_datum FROM yaci_store.address_utxo WHERE tx_hash = $1 AND owner_addr LIKE 'addr1x%' AND inline_datum IS NOT NULL LIMIT 1"
        )
        .bind(tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        // Fallback to pre-fetched treasury.utxo_history
        let inline_datum = match inline_datum {
            Some(d) => Some(d),
            None => sqlx::query_scalar::<_, Option<String>>(
                "SELECT inline_datum_cbor FROM treasury.utxo_history WHERE tx_hash = $1 AND address LIKE 'addr1x%' AND inline_datum_cbor IS NOT NULL LIMIT 1"
            )
            .bind(tx_hash)
            .fetch_optional(&self.pool)
            .await?
            .flatten(),
        };

        if let Some(datum_hex) = inline_datum {
            match crate::parsers::datum::parse_project_datum(&datum_hex) {
                Ok(parsed) => {
                    // Get non-withdrawn milestones ordered by milestone_order.
                    // Withdrawn milestones are consumed on-chain, so the datum only
                    // contains entries for non-withdrawn milestones. We must skip
                    // withdrawn rows to keep datum indices aligned with DB rows.
                    let milestone_ids: Vec<(i32,)> = sqlx::query_as(
                        "SELECT id FROM treasury.milestones WHERE project_db_id = $1 AND NOT archived AND NOT withdrawn ORDER BY milestone_order"
                    )
                    .bind(project_db_id)
                    .fetch_all(&self.pool)
                    .await?;

                    for (datum_idx, (db_id,)) in milestone_ids.iter().enumerate() {
                        if let Some(ms_datum) = parsed.milestones.get(datum_idx) {
                            sqlx::query(
                                "UPDATE treasury.milestones SET paused = $1 WHERE id = $2"
                            )
                            .bind(ms_datum.paused)
                            .bind(db_id)
                            .execute(&self.pool)
                            .await?;
                        }
                    }

                    // Update contract-level status: paused if ALL milestones paused
                    let all_paused = parsed.milestones.iter().all(|m| m.paused);
                    let any_paused = parsed.milestones.iter().any(|m| m.paused);
                    if all_paused && !parsed.milestones.is_empty() {
                        sqlx::query("UPDATE treasury.projects SET status = 'paused' WHERE id = $1")
                            .bind(project_db_id)
                            .execute(&self.pool)
                            .await?;
                    } else if !any_paused {
                        sqlx::query("UPDATE treasury.projects SET status = 'active' WHERE id = $1")
                            .bind(project_db_id)
                            .execute(&self.pool)
                            .await?;
                    }
                }
                Err(e) => {
                    tracing::debug!("Could not parse datum for pause/resume: {}", e);
                }
            }
        }

        Ok(())
    }
}

/// Extract milestone label and description from metadata fields.
///
/// On-chain TOM metadata typically has no `label` field on milestones and an empty
/// `description`. Instead, `acceptanceCriteria` contains structured text like:
///   "Milestone 2 - Documentation\nDeliverables: detailed description"
/// or with a project prefix:
///   "Ledger App Rewrite:\nMilestone 2 – Impl\nDeliverables: ..."
///
/// This function extracts a clean label and description:
/// - label: the milestone title (text before "\nDeliverables:" or first line)
/// - description: the deliverables text (after "Deliverables:"), or original description
fn extract_milestone_label_description(
    raw_label: Option<String>,
    raw_description: Option<String>,
    acceptance_criteria: &Option<String>,
) -> (Option<String>, Option<String>) {
    // If label is explicitly provided, use it (truncated to first line)
    if let Some(ref label) = raw_label {
        let label = label.lines().next().unwrap_or(label).trim().to_string();
        if !label.is_empty() {
            return (Some(label), raw_description);
        }
    }

    // No label — try to derive from acceptance_criteria, then fall back to
    // the first line of the description (covers `UTXO-*` projects whose
    // metadata uses `description` instead of `acceptanceCriteria`).
    // See KI-MIL-01 in docs/known-issues.md.
    let ac = match acceptance_criteria {
        Some(ac) if !ac.is_empty() => ac,
        _ => {
            if let Some(ref desc) = raw_description {
                let label = desc.lines().next().unwrap_or(desc).trim().to_string();
                if !label.is_empty() {
                    return (Some(label), raw_description);
                }
            }
            return (None, raw_description);
        }
    };

    // Look for "Deliverables:" separator (case-insensitive find)
    let deliverables_pos = ac.to_lowercase().find("\ndeliverables:");
    if let Some(pos) = deliverables_pos {
        let label = ac[..pos].trim().to_string();
        let desc_start = pos + 1; // skip the \n
        let deliverables = ac[desc_start..].trim().to_string();
        let description = if !deliverables.is_empty() {
            Some(deliverables)
        } else {
            raw_description
        };
        return (
            if label.is_empty() { None } else { Some(label) },
            description,
        );
    }

    // No "Deliverables:" marker — use first line as label
    let label = ac.lines().next().unwrap_or(ac).trim().to_string();
    (
        if label.is_empty() { None } else { Some(label) },
        raw_description,
    )
}

/// Extract milestone identifier hints from a complete/withdraw event body.
/// Used to disambiguate which vendor contract a tx belongs to when its inputs span
/// multiple project chains (e.g. fee/collateral pulled from a sibling contract).
/// Convert a metadata milestone key (`m-N` 0-indexed or `MS-N` 1-indexed) to
/// its canonical 1-indexed `milestone_order`. Returns `None` for unrecognised
/// formats. See `docs/known-issues.md` `KI-OC-01` for context.
fn canonical_milestone_order(key: &str) -> Option<i32> {
    if let Some(rest) = key.strip_prefix("m-") {
        rest.parse::<i32>().ok().map(|n| n + 1)
    } else if let Some(rest) = key.strip_prefix("MS-") {
        rest.parse::<i32>().ok()
    } else {
        None
    }
}

fn collect_milestone_id_hints(event_body: &Value) -> Vec<String> {
    let mut hints: Vec<String> = Vec::new();
    if let Some(obj) = event_body.get("milestones").and_then(|m| m.as_object()) {
        hints.extend(obj.keys().cloned());
    }
    if let Some(s) = event_body.get("milestone").and_then(|m| m.as_str()) {
        hints.push(s.to_string());
    }
    hints
}

/// Extract text from a field that might be a string or array
fn extract_text(obj: &Value, field: &str) -> Option<String> {
    extract_text_from_value(obj.get(field))
}

/// Extract text from a value that might be a string or array of 64-byte CIP-100 chunks.
/// Joining with "" (no separator) is correct for CIP-100: text is split at fixed byte
/// boundaries, so chunks are contiguous fragments that reconstruct the original text.
fn extract_text_from_value(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Array(arr)) => {
            let joined: String = arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("");
            if joined.is_empty() { None } else { Some(joined) }
        }
        Some(Value::Object(obj)) => {
            obj.get("label")
                .or_else(|| obj.get("name"))
                .and_then(|v| extract_text_from_value(Some(v)))
        }
        _ => None,
    }
}
