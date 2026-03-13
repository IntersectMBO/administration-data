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
        let name = extract_text(event_body, "label");
        let permissions = event_body.get("permissions").cloned();

        // Upsert treasury contract
        let treasury_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO treasury.treasury_contracts (contract_instance, name, publish_tx_hash, publish_time, permissions)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (contract_instance) DO UPDATE
                SET name = COALESCE(EXCLUDED.name, treasury.treasury_contracts.name),
                    publish_tx_hash = COALESCE(treasury.treasury_contracts.publish_tx_hash, EXCLUDED.publish_tx_hash),
                    publish_time = COALESCE(treasury.treasury_contracts.publish_time, EXCLUDED.publish_time),
                    permissions = COALESCE(EXCLUDED.permissions, treasury.treasury_contracts.permissions)
            RETURNING id
            "#
        )
        .bind(instance)
        .bind(&name)
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
        // TOM spec has no vendor.name field — vendor_name is not available in metadata
        let vendor_name: Option<String> = None;
        let vendor_address = event_body.get("vendor")
            .and_then(|v| extract_text_from_value(v.get("label")));
        let contract_url = extract_contract(event_body);
        let mut other_identifiers: Vec<String> = event_body.get("otherIdentifiers")
            .and_then(|o| o.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect::<Vec<_>>())
            .unwrap_or_default();
        other_identifiers.extend(extra_ids);
        let other_identifiers = if other_identifiers.is_empty() { None } else { Some(other_identifiers) };

        // Get contract address from fund tx output
        let contract_address: Option<String> = sqlx::query_scalar(
            "SELECT owner_addr FROM yaci_store.address_utxo WHERE tx_hash = $1 AND owner_addr LIKE 'addr1x%' LIMIT 1"
        )
        .bind(&event.tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        // Get initial amount from fund tx output
        let initial_amount: Option<i64> = sqlx::query_scalar(
            "SELECT lovelace_amount FROM yaci_store.address_utxo WHERE tx_hash = $1 AND owner_addr LIKE 'addr1x%' LIMIT 1"
        )
        .bind(&event.tx_hash)
        .fetch_optional(&self.pool)
        .await?;

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

        // Insert vendor contract
        let vendor_contract_id: i32 = sqlx::query_scalar(
            r#"
            INSERT INTO treasury.vendor_contracts (
                treasury_id, project_id, other_identifiers, project_name, description,
                vendor_name, vendor_address, contract_url, contract_address,
                fund_tx_hash, fund_slot, fund_block_time, initial_amount_lovelace, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 'active')
            ON CONFLICT (project_id) DO UPDATE
                SET project_name = COALESCE(EXCLUDED.project_name, treasury.vendor_contracts.project_name),
                    description = COALESCE(EXCLUDED.description, treasury.vendor_contracts.description)
            RETURNING id
            "#
        )
        .bind(treasury_id)
        .bind(project_id)
        .bind(&other_identifiers)
        .bind(&project_name)
        .bind(&description)
        .bind(&vendor_name)
        .bind(&vendor_address)
        .bind(&contract_url)
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
                        vendor_contract_id, milestone_id, milestone_order, label,
                        description, acceptance_criteria, amount_lovelace
                    )
                    VALUES ($1, $2, $3, $4, $5, $6, $7)
                    ON CONFLICT (vendor_contract_id, milestone_id) WHERE NOT archived DO NOTHING
                    "#
                )
                .bind(vendor_contract_id)
                .bind(milestone_id)
                .bind((idx + 1) as i32)
                .bind(&label)
                .bind(&description)
                .bind(&acceptance_criteria)
                .bind(amount)
                .execute(&self.pool)
                .await?;
        }

        self.insert_event_full(event, "fund", treasury_id, Some(vendor_contract_id), None, initial_amount, &None, &None, body).await?;

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
                    // Look up address and amount from yaci_store
                    let utxo_info: Option<(String, Option<i64>)> = sqlx::query_as(
                        "SELECT owner_addr, lovelace_amount FROM yaci_store.address_utxo WHERE tx_hash = $1 AND output_index = $2 LIMIT 1"
                    )
                    .bind(tx_hash)
                    .bind(output_index as i16)
                    .fetch_optional(&self.pool)
                    .await?;

                    let (address, lovelace_amount) = match utxo_info {
                        Some((addr, amt)) => (Some(addr), amt),
                        None => (None, None),
                    };
                    let address_type = address.as_ref().map(|a| {
                        if a.starts_with("addr1x") { "vendor_contract" } else { "vendor" }
                    });

                    // Record this UTXO with the vendor_contract_id for future event lookups
                    sqlx::query(
                        r#"
                        INSERT INTO treasury.utxos (tx_hash, output_index, vendor_contract_id, slot, block_number, address, address_type, lovelace_amount, spent)
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, false)
                        ON CONFLICT (tx_hash, output_index) DO UPDATE
                            SET address = COALESCE(EXCLUDED.address, treasury.utxos.address),
                                address_type = COALESCE(EXCLUDED.address_type, treasury.utxos.address_type),
                                lovelace_amount = COALESCE(EXCLUDED.lovelace_amount, treasury.utxos.lovelace_amount),
                                block_number = COALESCE(EXCLUDED.block_number, treasury.utxos.block_number)
                        "#
                    )
                    .bind(tx_hash)
                    .bind(output_index as i16)
                    .bind(vendor_contract_id)
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
        let inline_datum: Option<String> = sqlx::query_scalar(
            "SELECT inline_datum FROM yaci_store.address_utxo WHERE tx_hash = $1 AND owner_addr LIKE 'addr1x%' AND inline_datum IS NOT NULL LIMIT 1"
        )
        .bind(&event.tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(datum_hex) = inline_datum {
            match crate::parsers::datum::parse_vendor_contract_datum(&datum_hex) {
                Ok(parsed) => {
                    // Store vendor_payment_key_hash
                    sqlx::query(
                        "UPDATE treasury.vendor_contracts SET vendor_payment_key_hash = $1 WHERE id = $2"
                    )
                    .bind(&parsed.vendor_payment_key_hash)
                    .bind(vendor_contract_id)
                    .execute(&self.pool)
                    .await?;

                    // Update milestones with datum data (amount, time_limit, paused)
                    let milestone_rows: Vec<(i32, i32)> = sqlx::query_as(
                        "SELECT id, milestone_order FROM treasury.milestones WHERE vendor_contract_id = $1 AND NOT archived ORDER BY milestone_order"
                    )
                    .bind(vendor_contract_id)
                    .fetch_all(&self.pool)
                    .await?;

                    for (db_id, order) in &milestone_rows {
                        let datum_idx = (*order as usize).saturating_sub(1);
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
                        "UPDATE treasury.utxos SET inline_datum_cbor = $1 WHERE tx_hash = $2 AND vendor_contract_id = $3"
                    )
                    .bind(&datum_hex)
                    .bind(&event.tx_hash)
                    .bind(vendor_contract_id)
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

        // First try to get project_id from metadata (older format)
        let project_id_from_meta = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty());

        // Get vendor contract ID - either from metadata or by tracing tx chain
        let vendor_contract_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "SELECT id FROM treasury.vendor_contracts WHERE project_id = $1"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            // Trace back through transaction chain to find the project
            self.find_vendor_contract_from_inputs(&event.tx_hash).await?
        };

        let vendor_contract_id = match vendor_contract_id {
            Some(id) => id,
            None => {
                tracing::debug!("Could not find vendor contract for complete event {}", event.tx_hash);
                return Ok(());
            }
        };

        // Process completed milestones
        if let Some(milestones) = event_body.get("milestones") {
            // Milestones can be an object keyed by milestone_id
            if let Some(obj) = milestones.as_object() {
                for (milestone_id, milestone_data) in obj {
                    let description = extract_text_from_value(Some(milestone_data.get("description").unwrap_or(&Value::Null)));
                    let evidence = milestone_data.get("evidence").cloned();

                    let db_milestone_id: Option<i32> = sqlx::query_scalar(
                        r#"
                        UPDATE treasury.milestones
                        SET evidence_provided = TRUE,
                            complete_tx_hash = $1,
                            complete_time = $2,
                            complete_description = $3,
                            evidence = $4
                        WHERE vendor_contract_id = $5 AND milestone_id = $6 AND NOT archived
                        RETURNING id
                        "#
                    )
                    .bind(&event.tx_hash)
                    .bind(event.block_time)
                    .bind(&description)
                    .bind(&evidence)
                    .bind(vendor_contract_id)
                    .bind(milestone_id)
                    .fetch_optional(&self.pool)
                    .await?;

                    if let Some(mid) = db_milestone_id {
                        self.insert_event_full(event, "complete", None, Some(vendor_contract_id), Some(mid), None, &None, &None, body).await?;
                    }
                }
            }
        }

        // Also check for single milestone field (older format)
        if let Some(milestone_id) = event_body.get("milestone").and_then(|m| m.as_str()) {
            sqlx::query(
                r#"
                UPDATE treasury.milestones
                SET evidence_provided = TRUE,
                    complete_tx_hash = $1,
                    complete_time = $2
                WHERE vendor_contract_id = $3 AND milestone_id = $4 AND NOT archived
                "#
            )
            .bind(&event.tx_hash)
            .bind(event.block_time)
            .bind(vendor_contract_id)
            .bind(milestone_id)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Process a disburse event - treasury-level fund movement (does not touch milestones)
    async fn process_disburse(&self, event: &RawTomEvent, body: &Value, instance: &str) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);
        let destination = extract_text(event_body, "destination");

        // Disburse is a treasury-level operation — look up treasury_id, not vendor_contract_id
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

        // Get vendor contract ID - either from metadata or by tracing tx chain
        let vendor_contract_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "SELECT id FROM treasury.vendor_contracts WHERE project_id = $1"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            self.find_vendor_contract_from_inputs(&event.tx_hash).await?
        };

        if let Some(vc_id) = vendor_contract_id {
            // Get withdraw amount from tx outputs (non-script addresses)
            let withdraw_amount: Option<i64> = sqlx::query_scalar(
                "SELECT COALESCE(SUM(lovelace_amount)::bigint, 0) FROM yaci_store.address_utxo WHERE tx_hash = $1 AND owner_addr NOT LIKE 'addr1x%'"
            )
            .bind(&event.tx_hash)
            .fetch_optional(&self.pool)
            .await?;

            // Handle milestones object (plural, keyed by ID) — spec format
            if let Some(milestones) = event_body.get("milestones").and_then(|m| m.as_object()) {
                for (milestone_id, _milestone_data) in milestones {
                    let db_milestone_id: Option<i32> = sqlx::query_scalar(
                        r#"
                        UPDATE treasury.milestones
                        SET withdrawn = TRUE,
                            withdraw_tx_hash = $1,
                            withdraw_time = $2,
                            withdraw_amount = $3
                        WHERE vendor_contract_id = $4 AND milestone_id = $5 AND NOT archived
                        RETURNING id
                        "#
                    )
                    .bind(&event.tx_hash)
                    .bind(event.block_time)
                    .bind(withdraw_amount)
                    .bind(vc_id)
                    .bind(milestone_id)
                    .fetch_optional(&self.pool)
                    .await?;

                    if let Some(mid) = db_milestone_id {
                        self.insert_event_full(event, "withdraw", None, Some(vc_id), Some(mid), withdraw_amount, &None, &None, body).await?;
                    }
                }
            } else if let Some(milestone_id) = event_body.get("milestone").and_then(|m| m.as_str()) {
                // Handle singular milestone (legacy format)
                let db_milestone_id: Option<i32> = sqlx::query_scalar(
                    r#"
                    UPDATE treasury.milestones
                    SET withdrawn = TRUE,
                        withdraw_tx_hash = $1,
                        withdraw_time = $2,
                        withdraw_amount = $3
                    WHERE vendor_contract_id = $4 AND milestone_id = $5 AND NOT archived
                    RETURNING id
                    "#
                )
                .bind(&event.tx_hash)
                .bind(event.block_time)
                .bind(withdraw_amount)
                .bind(vc_id)
                .bind(milestone_id)
                .fetch_optional(&self.pool)
                .await?;

                self.insert_event_full(event, "withdraw", None, Some(vc_id), db_milestone_id, withdraw_amount, &None, &None, body).await?;
            } else {
                self.insert_event_full(event, "withdraw", None, Some(vc_id), None, withdraw_amount, &None, &None, body).await?;
            }
        } else {
            tracing::debug!("Could not find vendor contract for withdraw event {}", event.tx_hash);
        }

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
        let vendor_contract_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "UPDATE treasury.vendor_contracts SET status = 'paused' WHERE project_id = $1 RETURNING id"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            // Find via tx chain first, then update
            if let Some(vc_id) = self.find_vendor_contract_from_inputs(&event.tx_hash).await? {
                sqlx::query("UPDATE treasury.vendor_contracts SET status = 'paused' WHERE id = $1")
                    .bind(vc_id)
                    .execute(&self.pool)
                    .await?;
                Some(vc_id)
            } else {
                None
            }
        };

        if let Some(vc_id) = vendor_contract_id {
            // Also update per-milestone pause flags from output datum if available
            self.update_milestone_pause_from_datum(&event.tx_hash, vc_id).await?;

            self.insert_event_full(event, "pause", None, Some(vc_id), None, None, &reason, &None, body).await?;
        } else {
            tracing::debug!("Could not find vendor contract for pause event {}", event.tx_hash);
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
        let vendor_contract_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "UPDATE treasury.vendor_contracts SET status = 'active' WHERE project_id = $1 RETURNING id"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            if let Some(vc_id) = self.find_vendor_contract_from_inputs(&event.tx_hash).await? {
                sqlx::query("UPDATE treasury.vendor_contracts SET status = 'active' WHERE id = $1")
                    .bind(vc_id)
                    .execute(&self.pool)
                    .await?;
                Some(vc_id)
            } else {
                None
            }
        };

        if let Some(vc_id) = vendor_contract_id {
            // Also update per-milestone pause flags from output datum if available
            self.update_milestone_pause_from_datum(&event.tx_hash, vc_id).await?;

            self.insert_event_full(event, "resume", None, Some(vc_id), None, None, &None, &None, body).await?;
        } else {
            tracing::debug!("Could not find vendor contract for resume event {}", event.tx_hash);
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
        let vendor_contract_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "SELECT id FROM treasury.vendor_contracts WHERE project_id = $1"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            self.find_vendor_contract_from_inputs(&event.tx_hash).await?
        };

        if let Some(vc_id) = vendor_contract_id {
            // Update naming fields if present in modify metadata
            let project_name = extract_text(event_body, "label");
            let description = extract_text(event_body, "description");
            let vendor_address = event_body.get("vendor")
                .and_then(|v| extract_text_from_value(v.get("label")));
            let contract_url = extract_contract(event_body);

            sqlx::query(
                r#"
                UPDATE treasury.vendor_contracts
                SET project_name = COALESCE($1, project_name),
                    description = COALESCE($2, description),
                    vendor_address = COALESCE($3, vendor_address),
                    contract_url = COALESCE($4, contract_url)
                WHERE id = $5
                "#
            )
            .bind(&project_name)
            .bind(&description)
            .bind(&vendor_address)
            .bind(&contract_url)
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
                    WHERE vendor_contract_id = $3 AND NOT archived
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
                            vendor_contract_id, milestone_id, milestone_order, label,
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
                        WHERE vendor_contract_id = $2 AND milestone_id = $3 AND archived AND superseded_by IS NULL
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
            tracing::debug!("Could not find vendor contract for modify event {}", event.tx_hash);
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
        let vendor_contract_id: Option<i32> = if let Some(pid) = project_id_from_meta {
            sqlx::query_scalar(
                "UPDATE treasury.vendor_contracts SET status = 'cancelled' WHERE project_id = $1 RETURNING id"
            )
            .bind(pid)
            .fetch_optional(&self.pool)
            .await?
        } else {
            if let Some(vc_id) = self.find_vendor_contract_from_inputs(&event.tx_hash).await? {
                sqlx::query("UPDATE treasury.vendor_contracts SET status = 'cancelled' WHERE id = $1")
                    .bind(vc_id)
                    .execute(&self.pool)
                    .await?;
                Some(vc_id)
            } else {
                None
            }
        };

        if let Some(vc_id) = vendor_contract_id {
            self.insert_event_full(event, "cancel", None, Some(vc_id), None, None, &reason, &None, body).await?;
        } else {
            tracing::debug!("Could not find vendor contract for cancel event {}", event.tx_hash);
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
        vendor_contract_id: Option<i32>,
        milestone_id: Option<i32>,
        amount_lovelace: Option<i64>,
        reason: &Option<String>,
        destination: &Option<String>,
        body: &Value,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO treasury.events (
                tx_hash, slot, block_number, block_time, event_type,
                treasury_id, vendor_contract_id, milestone_id,
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
        .bind(vendor_contract_id)
        .bind(milestone_id)
        .bind(amount_lovelace)
        .bind(reason)
        .bind(destination)
        .bind(body)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Find vendor_contract_id by looking up input UTXOs in our treasury.utxos tracking table.
    /// When a fund event is processed, its output UTXOs are recorded with the vendor_contract_id.
    /// Subsequent events (complete/withdraw/etc) spend those UTXOs, so we can find the project
    /// by looking at which tracked UTXOs are being spent as inputs.
    async fn find_vendor_contract_from_inputs(&self, tx_hash: &str) -> anyhow::Result<Option<i32>> {
        // Get the inputs to this transaction
        let inputs: Vec<(String, i16)> = sqlx::query_as(
            r#"
            SELECT tx_hash, output_index::smallint
            FROM yaci_store.tx_input
            WHERE spent_tx_hash = $1
            "#
        )
        .bind(tx_hash)
        .fetch_all(&self.pool)
        .await?;

        // Look up each input in our tracked UTXOs
        for (input_tx_hash, input_output_index) in &inputs {
            let vendor_contract_id: Option<i32> = sqlx::query_scalar(
                r#"
                SELECT vendor_contract_id
                FROM treasury.utxos
                WHERE tx_hash = $1 AND output_index = $2 AND vendor_contract_id IS NOT NULL
                "#
            )
            .bind(input_tx_hash)
            .bind(input_output_index)
            .fetch_optional(&self.pool)
            .await?;

            if let Some(vc_id) = vendor_contract_id {
                // Mark this UTXO as spent and record the new outputs
                sqlx::query(
                    r#"
                    UPDATE treasury.utxos
                    SET spent = true, spent_tx_hash = $1
                    WHERE tx_hash = $2 AND output_index = $3
                    "#
                )
                .bind(tx_hash)
                .bind(input_tx_hash)
                .bind(input_output_index)
                .execute(&self.pool)
                .await?;

                // Record the outputs of this transaction with the same vendor_contract_id
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
                            // Look up address, amount, and inline datum from yaci_store
                            let utxo_info: Option<(String, Option<i64>, Option<String>)> = sqlx::query_as(
                                "SELECT owner_addr, lovelace_amount, inline_datum FROM yaci_store.address_utxo WHERE tx_hash = $1 AND output_index = $2 LIMIT 1"
                            )
                            .bind(out_tx_hash)
                            .bind(output_index as i16)
                            .fetch_optional(&self.pool)
                            .await?;

                            let (address, lovelace_amount, out_datum) = match utxo_info {
                                Some((addr, amt, datum)) => (Some(addr), amt, datum),
                                None => (None, None, None),
                            };
                            let address_type = address.as_ref().map(|a| {
                                if a.starts_with("addr1x") { "vendor_contract" } else { "vendor" }
                            });

                            sqlx::query(
                                r#"
                                INSERT INTO treasury.utxos (tx_hash, output_index, vendor_contract_id, address, address_type, lovelace_amount, inline_datum_cbor, spent)
                                VALUES ($1, $2, $3, $4, $5, $6, $7, false)
                                ON CONFLICT (tx_hash, output_index) DO UPDATE
                                    SET vendor_contract_id = EXCLUDED.vendor_contract_id,
                                        address = COALESCE(EXCLUDED.address, treasury.utxos.address),
                                        address_type = COALESCE(EXCLUDED.address_type, treasury.utxos.address_type),
                                        lovelace_amount = COALESCE(EXCLUDED.lovelace_amount, treasury.utxos.lovelace_amount),
                                        inline_datum_cbor = COALESCE(EXCLUDED.inline_datum_cbor, treasury.utxos.inline_datum_cbor)
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

                return Ok(Some(vc_id));
            }
        }

        tracing::debug!("No tracked UTXO found for tx {} inputs", tx_hash);
        Ok(None)
    }

    /// Update per-milestone pause flags from the output datum of a transaction
    async fn update_milestone_pause_from_datum(&self, tx_hash: &str, vendor_contract_id: i32) -> anyhow::Result<()> {
        // Query inline datum from the tx output at the vendor contract address
        let inline_datum: Option<String> = sqlx::query_scalar(
            "SELECT inline_datum FROM yaci_store.address_utxo WHERE tx_hash = $1 AND owner_addr LIKE 'addr1x%' AND inline_datum IS NOT NULL LIMIT 1"
        )
        .bind(tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(datum_hex) = inline_datum {
            match crate::parsers::datum::parse_vendor_contract_datum(&datum_hex) {
                Ok(parsed) => {
                    // Get milestones ordered by milestone_order
                    let milestone_ids: Vec<(i32, i32)> = sqlx::query_as(
                        "SELECT id, milestone_order FROM treasury.milestones WHERE vendor_contract_id = $1 AND NOT archived ORDER BY milestone_order"
                    )
                    .bind(vendor_contract_id)
                    .fetch_all(&self.pool)
                    .await?;

                    for (db_id, order) in &milestone_ids {
                        let datum_idx = (*order as usize).saturating_sub(1);
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
                        sqlx::query("UPDATE treasury.vendor_contracts SET status = 'paused' WHERE id = $1")
                            .bind(vendor_contract_id)
                            .execute(&self.pool)
                            .await?;
                    } else if !any_paused {
                        sqlx::query("UPDATE treasury.vendor_contracts SET status = 'active' WHERE id = $1")
                            .bind(vendor_contract_id)
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

/// Extract contract URL from a field that might be a string or an object with anchorUrl
fn extract_contract(event_body: &Value) -> Option<String> {
    match event_body.get("contract") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Object(obj)) => obj.get("anchorUrl")
            .and_then(|u| u.as_str())
            .map(String::from),
        _ => None,
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

    // No label — try to derive from acceptance_criteria
    let ac = match acceptance_criteria {
        Some(ac) if !ac.is_empty() => ac,
        _ => return (None, raw_description),
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
