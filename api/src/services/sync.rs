//! Background sync service for TOM events
//!
//! Periodically fetches new TOM (Treasury Oversight Metadata) events from
//! yaci_store.transaction_metadata and processes them into the normalized
//! treasury schema.

use sqlx::PgPool;
use std::time::Duration;

use super::event_processor::EventProcessor;

/// Run the background sync loop
pub async fn run_sync_loop(pool: PgPool) {
    let processor = EventProcessor::new(pool.clone());

    // Wait for yaci_store tables to be created by the indexer's Flyway migrations
    tracing::info!("Waiting for yaci_store tables to be available...");
    for attempt in 1..=30 {
        let table_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'yaci_store' AND table_name = 'transaction_metadata')"
        )
        .fetch_one(&pool)
        .await
        .unwrap_or(false);

        if table_exists {
            tracing::info!("yaci_store tables are ready");
            break;
        }

        if attempt == 30 {
            tracing::warn!("yaci_store tables not found after 30 attempts, proceeding anyway");
            break;
        }

        tracing::info!("yaci_store tables not ready yet, retrying in 5s... (attempt {}/30)", attempt);
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    // Install / refresh the trigger that captures every script-address UTXO
    // into treasury.utxo_history before YACI Store can prune it. Idempotent.
    if let Err(e) = install_utxo_history_triggers(&pool).await {
        tracing::warn!("Failed to install utxo_history triggers (non-fatal): {}", e);
    } else {
        tracing::info!("utxo_history triggers installed");
    }

    // Initial sync: process all events from beginning
    tracing::info!("Starting initial TOM event sync...");
    if let Err(e) = processor.sync_all_events().await {
        tracing::error!("Initial sync failed: {}", e);
    }

    tracing::info!("Initial sync complete. Starting continuous sync loop.");

    // Continuous sync loop
    loop {
        tokio::time::sleep(Duration::from_secs(15)).await;

        if let Err(e) = sync_new_events(&pool, &processor).await {
            tracing::error!("Sync error: {}", e);
        }
    }
}

/// Fetch and process new TOM events since last sync
async fn sync_new_events(pool: &PgPool, processor: &EventProcessor) -> anyhow::Result<()> {
    // Get last synced slot
    let last_slot: i64 = sqlx::query_scalar(
        "SELECT last_slot FROM treasury.sync_status WHERE sync_type = 'events'"
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    // Fetch new TOM events from yaci_store
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
        WHERE m.label = '1694' AND m.slot > $1
        ORDER BY m.slot ASC, m.tx_hash ASC
        LIMIT 1000
        "#
    )
    .bind(last_slot)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        // Bump updated_at on idle ticks so /api/v1/statistics shows a live heartbeat.
        // Closes KI-SY-01 (`updated_at` doesn't bump on idle ticks).
        sqlx::query(
            "UPDATE treasury.sync_status SET updated_at = NOW() WHERE sync_type = 'events'"
        )
        .execute(pool)
        .await?;
        return Ok(());
    }

    tracing::info!("Processing {} new TOM events", rows.len());

    // Pre-fetch UTXOs from yaci_store into treasury.utxo_history before processing.
    // This captures UTXO data before YACI Store can prune spent UTXOs (~2160 blocks).
    let tx_hashes: Vec<String> = rows.iter().map(|r| r.tx_hash.clone()).collect();
    if let Err(e) = processor.pre_fetch_utxos(&tx_hashes).await {
        tracing::warn!("UTXO pre-fetch failed (non-fatal): {}", e);
    }

    let mut last_processed_slot = last_slot;
    let mut last_processed_tx = String::new();
    let mut last_block = 0i64;

    for row in rows {
        if let Err(e) = processor.process_event(&row).await {
            tracing::error!("Failed to process event {}: {}", row.tx_hash, e);
            continue;
        }

        last_processed_slot = row.slot.unwrap_or(last_processed_slot);
        last_block = row.block_number.unwrap_or(last_block);
        last_processed_tx = row.tx_hash.clone();
    }

    // Update sync status
    sqlx::query(
        r#"
        UPDATE treasury.sync_status
        SET last_slot = $1, last_block = $2, last_tx_hash = $3, updated_at = NOW()
        WHERE sync_type = 'events'
        "#
    )
    .bind(last_processed_slot)
    .bind(last_block)
    .bind(&last_processed_tx)
    .execute(pool)
    .await?;

    Ok(())
}

/// Install the Postgres triggers that mirror `yaci_store.address_utxo` and
/// `yaci_store.tx_input` into `treasury.utxo_history`. Idempotent.
///
/// Why this exists: YACI Store prunes spent UTXOs from `address_utxo` after
/// ~2160 blocks (~10 days). The trigger fires synchronously inside YACI's
/// INSERT transaction, so by the time pruning runs we already have a copy.
/// This solves the cold-replay limitation documented as `KI-CR-01` and
/// closes `KI-EVT-01` / `KI-VND-04` / `KI-UTX-01`.
async fn install_utxo_history_triggers(pool: &PgPool) -> anyhow::Result<()> {
    // Capture every script-address (addr1x*) UTXO created by YACI Store.
    sqlx::query(
        r#"
        CREATE OR REPLACE FUNCTION treasury.capture_utxo_history()
        RETURNS TRIGGER AS $$
        BEGIN
            IF NEW.owner_addr LIKE 'addr1x%' THEN
                INSERT INTO treasury.utxo_history (
                    tx_hash, output_index, address, lovelace_amount,
                    inline_datum_cbor, slot, block_number
                ) VALUES (
                    NEW.tx_hash, NEW.output_index, NEW.owner_addr,
                    NEW.lovelace_amount, NEW.inline_datum, NEW.slot, NEW.block
                )
                ON CONFLICT (tx_hash, output_index) DO UPDATE
                    SET address = COALESCE(EXCLUDED.address, treasury.utxo_history.address),
                        lovelace_amount = COALESCE(EXCLUDED.lovelace_amount, treasury.utxo_history.lovelace_amount),
                        inline_datum_cbor = COALESCE(EXCLUDED.inline_datum_cbor, treasury.utxo_history.inline_datum_cbor),
                        slot = COALESCE(EXCLUDED.slot, treasury.utxo_history.slot),
                        block_number = COALESCE(EXCLUDED.block_number, treasury.utxo_history.block_number);
            END IF;
            RETURN NEW;
        END;
        $$ LANGUAGE plpgsql;
        "#,
    )
    .execute(pool)
    .await?;

    // CREATE TRIGGER takes a SHARE ROW EXCLUSIVE lock; if YACI Store is mid-batch
    // it will block. Skip the create when the trigger already exists so subsequent
    // restarts are non-blocking.
    sqlx::query(
        r#"
        DO $$
        BEGIN
            IF NOT EXISTS (
                SELECT 1 FROM information_schema.triggers
                WHERE event_object_schema = 'yaci_store'
                  AND event_object_table = 'address_utxo'
                  AND trigger_name = 'capture_address_utxo'
            ) THEN
                CREATE TRIGGER capture_address_utxo
                AFTER INSERT OR UPDATE ON yaci_store.address_utxo
                FOR EACH ROW EXECUTE FUNCTION treasury.capture_utxo_history();
            END IF;
        END $$;
        "#,
    )
    .execute(pool)
    .await?;

    // Mark UTXOs as spent when YACI Store records the spending tx_input row.
    sqlx::query(
        r#"
        CREATE OR REPLACE FUNCTION treasury.mark_utxo_spent()
        RETURNS TRIGGER AS $$
        BEGIN
            UPDATE treasury.utxo_history
            SET spent = TRUE,
                spent_tx_hash = NEW.spent_tx_hash,
                spent_slot = NEW.spent_at_slot
            WHERE tx_hash = NEW.tx_hash AND output_index = NEW.output_index;
            RETURN NEW;
        END;
        $$ LANGUAGE plpgsql;
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        DO $$
        BEGIN
            IF NOT EXISTS (
                SELECT 1 FROM information_schema.triggers
                WHERE event_object_schema = 'yaci_store'
                  AND event_object_table = 'tx_input'
                  AND trigger_name = 'mark_utxo_spent'
            ) THEN
                CREATE TRIGGER mark_utxo_spent
                AFTER INSERT ON yaci_store.tx_input
                FOR EACH ROW EXECUTE FUNCTION treasury.mark_utxo_spent();
            END IF;
        END $$;
        "#,
    )
    .execute(pool)
    .await?;

    // One-shot backfill: copy any address_utxo rows already present (covers
    // the case where YACI Store ingested rows before the trigger was armed).
    sqlx::query(
        r#"
        INSERT INTO treasury.utxo_history (
            tx_hash, output_index, address, lovelace_amount,
            inline_datum_cbor, slot, block_number
        )
        SELECT au.tx_hash, au.output_index, au.owner_addr,
               au.lovelace_amount, au.inline_datum, au.slot, au.block
        FROM yaci_store.address_utxo au
        WHERE au.owner_addr LIKE 'addr1x%'
        ON CONFLICT (tx_hash, output_index) DO NOTHING
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Raw TOM event from yaci_store
#[derive(Debug, sqlx::FromRow)]
pub struct RawTomEvent {
    pub tx_hash: String,
    pub slot: Option<i64>,
    pub body: Option<serde_json::Value>,
    pub block_number: Option<i64>,
    pub block_time: Option<i64>,
}
