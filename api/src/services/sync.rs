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

    // Initial sync: process all events from beginning
    tracing::info!("Starting initial TOM event sync...");
    if let Err(e) = processor.sync_all_events().await {
        tracing::error!("Initial sync failed: {}", e);
    }

    // Sync UTXOs for tracked addresses
    tracing::info!("Syncing UTXOs for tracked addresses...");
    if let Err(e) = processor.sync_utxos().await {
        tracing::error!("UTXO sync failed: {}", e);
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
        ORDER BY m.slot ASC
        LIMIT 1000
        "#
    )
    .bind(last_slot)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    tracing::info!("Processing {} new TOM events", rows.len());

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

    // Also sync any new UTXOs
    processor.sync_utxos().await?;

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
