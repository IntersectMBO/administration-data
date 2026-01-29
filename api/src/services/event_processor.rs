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
            ORDER BY m.slot ASC
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
            "disburse" => self.process_disburse(event, body).await?,
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
        self.insert_event(event, "publish", Some(treasury_id), None, None, body).await?;

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

        self.insert_event(event, "initialize", Some(treasury_id), None, None, body).await?;

        Ok(())
    }

    /// Process a fund event - create vendor contract and milestones
    async fn process_fund(&self, event: &RawTomEvent, body: &Value, instance: &str) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);

        let project_id = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .unwrap_or("");

        if project_id.is_empty() {
            return Ok(());
        }

        let project_name = extract_text(event_body, "label");
        let description = extract_text(event_body, "description");
        let vendor_name = event_body.get("vendor")
            .and_then(|v| v.get("name"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());
        let vendor_address = event_body.get("vendor")
            .and_then(|v| extract_text_from_value(v.get("label")));
        let contract_url = event_body.get("contract")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());
        let other_identifiers = event_body.get("otherIdentifiers")
            .and_then(|o| o.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect::<Vec<_>>());

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

        // Process milestones
        if let Some(milestones) = event_body.get("milestones").and_then(|m| m.as_array()) {
            for (idx, milestone) in milestones.iter().enumerate() {
                let default_id = format!("m-{}", idx);
                let milestone_id = milestone.get("identifier")
                    .and_then(|i| i.as_str())
                    .unwrap_or(&default_id);
                let label = extract_text_from_value(Some(milestone.get("label").unwrap_or(&Value::Null)));
                let description = extract_text_from_value(Some(milestone.get("description").unwrap_or(&Value::Null)));
                let acceptance_criteria = extract_text_from_value(Some(milestone.get("acceptanceCriteria").unwrap_or(&Value::Null)));
                let amount = milestone.get("amount")
                    .and_then(|a| a.as_i64());

                sqlx::query(
                    r#"
                    INSERT INTO treasury.milestones (
                        vendor_contract_id, milestone_id, milestone_order, label,
                        description, acceptance_criteria, amount_lovelace, status
                    )
                    VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending')
                    ON CONFLICT (vendor_contract_id, milestone_id) DO NOTHING
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
        }

        self.insert_event(event, "fund", treasury_id, Some(vendor_contract_id), None, body).await?;

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
                    // Record this UTXO with the vendor_contract_id for future event lookups
                    sqlx::query(
                        r#"
                        INSERT INTO treasury.utxos (tx_hash, output_index, vendor_contract_id, slot, spent)
                        VALUES ($1, $2, $3, $4, false)
                        ON CONFLICT (tx_hash, output_index) DO NOTHING
                        "#
                    )
                    .bind(tx_hash)
                    .bind(output_index as i16)
                    .bind(vendor_contract_id)
                    .bind(event.slot)
                    .execute(&self.pool)
                    .await?;
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
                        SET status = 'completed',
                            complete_tx_hash = $1,
                            complete_time = $2,
                            complete_description = $3,
                            evidence = $4
                        WHERE vendor_contract_id = $5 AND milestone_id = $6
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
                        self.insert_event(event, "complete", None, Some(vendor_contract_id), Some(mid), body).await?;
                    }
                }
            }
        }

        // Also check for single milestone field (older format)
        if let Some(milestone_id) = event_body.get("milestone").and_then(|m| m.as_str()) {
            sqlx::query(
                r#"
                UPDATE treasury.milestones
                SET status = 'completed',
                    complete_tx_hash = $1,
                    complete_time = $2
                WHERE vendor_contract_id = $3 AND milestone_id = $4 AND status = 'pending'
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

    /// Process a disburse event - update milestone status
    async fn process_disburse(&self, event: &RawTomEvent, body: &Value) -> anyhow::Result<()> {
        let event_body = body.get("body").unwrap_or(body);

        let project_id_from_meta = event_body.get("identifier")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty());

        let destination = extract_text(event_body, "destination");

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

        // Get disbursed amount from tx outputs - cast SUM to BIGINT
        let disburse_amount: Option<i64> = sqlx::query_scalar(
            "SELECT COALESCE(SUM(lovelace_amount)::bigint, 0) FROM yaci_store.address_utxo WHERE tx_hash = $1 AND owner_addr NOT LIKE 'addr1x%'"
        )
        .bind(&event.tx_hash)
        .fetch_optional(&self.pool)
        .await?;

        // Check for milestone field and update if present
        let db_milestone_id: Option<i32> = if let (Some(vc_id), Some(milestone_id)) = (vendor_contract_id, event_body.get("milestone").and_then(|m| m.as_str())) {
            sqlx::query_scalar(
                r#"
                UPDATE treasury.milestones
                SET status = 'disbursed',
                    disburse_tx_hash = $1,
                    disburse_time = $2,
                    disburse_amount = $3
                WHERE vendor_contract_id = $4 AND milestone_id = $5
                RETURNING id
                "#
            )
            .bind(&event.tx_hash)
            .bind(event.block_time)
            .bind(disburse_amount)
            .bind(vc_id)
            .bind(milestone_id)
            .fetch_optional(&self.pool)
            .await?
        } else {
            None
        };

        // Always insert the disburse event (may be treasury-level without vendor_contract)
        self.insert_event_with_destination(event, "disburse", None, vendor_contract_id, db_milestone_id, &destination, body).await?;

        Ok(())
    }

    /// Process a withdraw event
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
            self.insert_event(event, "withdraw", None, Some(vc_id), None, body).await?;
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
            self.insert_event_with_reason(event, "pause", None, Some(vc_id), None, &reason, body).await?;
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
            self.insert_event(event, "resume", None, Some(vc_id), None, body).await?;
        } else {
            tracing::debug!("Could not find vendor contract for resume event {}", event.tx_hash);
        }

        Ok(())
    }

    /// Process a modify event - update vendor contract
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
            self.insert_event_with_reason(event, "modify", None, Some(vc_id), None, &reason, body).await?;
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
            self.insert_event_with_reason(event, "cancel", None, Some(vc_id), None, &reason, body).await?;
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

        self.insert_event(event, "sweep", treasury_id, None, None, body).await?;

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

        self.insert_event(event, "reorganize", treasury_id, None, None, body).await?;

        Ok(())
    }

    /// Insert an event record
    async fn insert_event(
        &self,
        event: &RawTomEvent,
        event_type: &str,
        treasury_id: Option<i32>,
        vendor_contract_id: Option<i32>,
        milestone_id: Option<i32>,
        body: &Value,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO treasury.events (
                tx_hash, slot, block_number, block_time, event_type,
                treasury_id, vendor_contract_id, milestone_id, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (tx_hash) DO NOTHING
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
        .bind(body)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert an event with reason field
    async fn insert_event_with_reason(
        &self,
        event: &RawTomEvent,
        event_type: &str,
        treasury_id: Option<i32>,
        vendor_contract_id: Option<i32>,
        milestone_id: Option<i32>,
        reason: &Option<String>,
        body: &Value,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO treasury.events (
                tx_hash, slot, block_number, block_time, event_type,
                treasury_id, vendor_contract_id, milestone_id, reason, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (tx_hash) DO NOTHING
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
        .bind(reason)
        .bind(body)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Insert an event with destination field
    async fn insert_event_with_destination(
        &self,
        event: &RawTomEvent,
        event_type: &str,
        treasury_id: Option<i32>,
        vendor_contract_id: Option<i32>,
        milestone_id: Option<i32>,
        destination: &Option<String>,
        body: &Value,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO treasury.events (
                tx_hash, slot, block_number, block_time, event_type,
                treasury_id, vendor_contract_id, milestone_id, destination, metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (tx_hash) DO NOTHING
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
                            sqlx::query(
                                r#"
                                INSERT INTO treasury.utxos (tx_hash, output_index, vendor_contract_id, spent)
                                VALUES ($1, $2, $3, false)
                                ON CONFLICT (tx_hash, output_index) DO UPDATE
                                    SET vendor_contract_id = EXCLUDED.vendor_contract_id
                                "#
                            )
                            .bind(out_tx_hash)
                            .bind(output_index as i16)
                            .bind(vc_id)
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

    /// Sync UTXOs for all tracked addresses
    pub async fn sync_utxos(&self) -> anyhow::Result<()> {
        // Get all contract addresses (both treasury and vendor)
        let addresses: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT contract_address FROM treasury.treasury_contracts WHERE contract_address IS NOT NULL
            UNION
            SELECT contract_address FROM treasury.vendor_contracts WHERE contract_address IS NOT NULL
            UNION
            SELECT vendor_address FROM treasury.vendor_contracts WHERE vendor_address IS NOT NULL
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        for address in addresses {
            self.sync_address_utxos(&address).await?;
        }

        Ok(())
    }

    /// Sync UTXOs for a specific address
    async fn sync_address_utxos(&self, address: &str) -> anyhow::Result<()> {
        // Determine address type and get vendor_contract_id if applicable
        let vendor_contract_id: Option<i32> = sqlx::query_scalar(
            "SELECT id FROM treasury.vendor_contracts WHERE contract_address = $1 OR vendor_address = $1"
        )
        .bind(address)
        .fetch_optional(&self.pool)
        .await?;

        let address_type = if address.starts_with("addr1x") {
            if vendor_contract_id.is_some() { "vendor_contract" } else { "treasury" }
        } else {
            "vendor"
        };

        // Get UTXOs from yaci_store
        let utxos = sqlx::query_as::<_, (String, i16, i64, i64, Option<i64>)>(
            r#"
            SELECT tx_hash, output_index::smallint, lovelace_amount, slot, block as block_number
            FROM yaci_store.address_utxo
            WHERE owner_addr = $1
            "#
        )
        .bind(address)
        .fetch_all(&self.pool)
        .await?;

        for (tx_hash, output_index, lovelace_amount, slot, block_number) in utxos {
            sqlx::query(
                r#"
                INSERT INTO treasury.utxos (
                    tx_hash, output_index, address, address_type,
                    vendor_contract_id, lovelace_amount, slot, block_number, spent
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, false)
                ON CONFLICT (tx_hash, output_index) DO NOTHING
                "#
            )
            .bind(&tx_hash)
            .bind(output_index)
            .bind(address)
            .bind(address_type)
            .bind(vendor_contract_id)
            .bind(lovelace_amount)
            .bind(slot)
            .bind(block_number)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }
}

/// Extract text from a field that might be a string or array
fn extract_text(obj: &Value, field: &str) -> Option<String> {
    extract_text_from_value(obj.get(field))
}

/// Extract text from a value that might be a string or array
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
        _ => None,
    }
}
