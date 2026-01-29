// Database connection utilities

use sqlx::PgPool;

/// Initialize the administration schema if it doesn't exist
/// This ensures all required tables, indexes, and views are created
pub async fn init_administration_schema(pool: &PgPool) -> Result<(), sqlx::Error> {
    tracing::info!("Initializing administration schema...");

    // Create schema
    sqlx::query("CREATE SCHEMA IF NOT EXISTS treasury")
        .execute(pool)
        .await?;

    // Create treasury_contracts table
    sqlx::query(r#"
        CREATE TABLE IF NOT EXISTS treasury.treasury_contracts (
            id SERIAL PRIMARY KEY,
            contract_instance TEXT UNIQUE NOT NULL,
            contract_address TEXT,
            stake_credential TEXT,
            name TEXT,
            publish_tx_hash VARCHAR(64),
            publish_time BIGINT,
            initialized_tx_hash VARCHAR(64),
            initialized_at BIGINT,
            permissions JSONB,
            status TEXT DEFAULT 'active',
            created_at TIMESTAMPTZ DEFAULT NOW(),
            updated_at TIMESTAMPTZ DEFAULT NOW()
        )
    "#).execute(pool).await?;

    // Create vendor_contracts table
    sqlx::query(r#"
        CREATE TABLE IF NOT EXISTS treasury.vendor_contracts (
            id SERIAL PRIMARY KEY,
            treasury_id INT REFERENCES treasury.treasury_contracts(id),
            project_id TEXT UNIQUE NOT NULL,
            other_identifiers TEXT[],
            project_name TEXT,
            description TEXT,
            vendor_name TEXT,
            vendor_address TEXT,
            contract_url TEXT,
            contract_address TEXT,
            fund_tx_hash VARCHAR(64) NOT NULL,
            fund_slot BIGINT,
            fund_block_time BIGINT,
            initial_amount_lovelace BIGINT,
            status TEXT DEFAULT 'active',
            created_at TIMESTAMPTZ DEFAULT NOW(),
            updated_at TIMESTAMPTZ DEFAULT NOW()
        )
    "#).execute(pool).await?;

    // Create milestones table
    sqlx::query(r#"
        CREATE TABLE IF NOT EXISTS treasury.milestones (
            id SERIAL PRIMARY KEY,
            vendor_contract_id INT NOT NULL REFERENCES treasury.vendor_contracts(id) ON DELETE CASCADE,
            milestone_id TEXT NOT NULL,
            milestone_order INT NOT NULL,
            label TEXT,
            description TEXT,
            acceptance_criteria TEXT,
            amount_lovelace BIGINT,
            status TEXT DEFAULT 'pending',
            complete_tx_hash VARCHAR(64),
            complete_time BIGINT,
            complete_description TEXT,
            evidence JSONB,
            disburse_tx_hash VARCHAR(64),
            disburse_time BIGINT,
            disburse_amount BIGINT,
            created_at TIMESTAMPTZ DEFAULT NOW(),
            updated_at TIMESTAMPTZ DEFAULT NOW(),
            UNIQUE(vendor_contract_id, milestone_id)
        )
    "#).execute(pool).await?;

    // Create events table
    sqlx::query(r#"
        CREATE TABLE IF NOT EXISTS treasury.events (
            id SERIAL PRIMARY KEY,
            tx_hash VARCHAR(64) UNIQUE NOT NULL,
            slot BIGINT,
            block_number BIGINT,
            block_time BIGINT,
            event_type TEXT NOT NULL,
            treasury_id INT REFERENCES treasury.treasury_contracts(id),
            vendor_contract_id INT REFERENCES treasury.vendor_contracts(id),
            milestone_id INT REFERENCES treasury.milestones(id),
            amount_lovelace BIGINT,
            reason TEXT,
            destination TEXT,
            metadata JSONB,
            created_at TIMESTAMPTZ DEFAULT NOW()
        )
    "#).execute(pool).await?;

    // Create utxos table
    sqlx::query(r#"
        CREATE TABLE IF NOT EXISTS treasury.utxos (
            id SERIAL PRIMARY KEY,
            tx_hash VARCHAR(64) NOT NULL,
            output_index SMALLINT NOT NULL,
            address TEXT,
            address_type TEXT,
            vendor_contract_id INT REFERENCES treasury.vendor_contracts(id),
            lovelace_amount BIGINT,
            slot BIGINT,
            block_number BIGINT,
            spent BOOLEAN DEFAULT FALSE,
            spent_tx_hash VARCHAR(64),
            spent_slot BIGINT,
            UNIQUE(tx_hash, output_index)
        )
    "#).execute(pool).await?;

    // Create sync_status table
    sqlx::query(r#"
        CREATE TABLE IF NOT EXISTS treasury.sync_status (
            id SERIAL PRIMARY KEY,
            sync_type TEXT UNIQUE NOT NULL,
            last_slot BIGINT DEFAULT 0,
            last_block BIGINT,
            last_tx_hash VARCHAR(64),
            updated_at TIMESTAMPTZ DEFAULT NOW()
        )
    "#).execute(pool).await?;

    // Insert initial sync status records
    sqlx::query(r#"
        INSERT INTO treasury.sync_status (sync_type, last_slot)
        VALUES ('events', 0), ('utxos', 0)
        ON CONFLICT (sync_type) DO NOTHING
    "#).execute(pool).await?;

    // Create indexes
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_treasury_instance ON treasury.treasury_contracts(contract_instance)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_treasury_address ON treasury.treasury_contracts(contract_address)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_treasury_status ON treasury.treasury_contracts(status)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vendor_treasury ON treasury.vendor_contracts(treasury_id)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vendor_project_id ON treasury.vendor_contracts(project_id)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vendor_status ON treasury.vendor_contracts(status)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vendor_fund_time ON treasury.vendor_contracts(fund_block_time DESC)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_vendor_contract_address ON treasury.vendor_contracts(contract_address)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_milestone_vendor ON treasury.milestones(vendor_contract_id)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_milestone_status ON treasury.milestones(status)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_milestone_order ON treasury.milestones(vendor_contract_id, milestone_order)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_type ON treasury.events(event_type)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_vendor ON treasury.events(vendor_contract_id)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_treasury ON treasury.events(treasury_id)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_slot ON treasury.events(slot DESC)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_block_time ON treasury.events(block_time DESC)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_utxo_address ON treasury.utxos(address)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_utxo_vendor ON treasury.utxos(vendor_contract_id)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_utxo_unspent ON treasury.utxos(address) WHERE NOT spent").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_utxo_slot ON treasury.utxos(slot DESC)").execute(pool).await?;

    // Create additional indexes for new views
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_utxo_vendor_unspent ON treasury.utxos(vendor_contract_id) WHERE NOT spent").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_milestone ON treasury.events(milestone_id)").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_event_type_time ON treasury.events(event_type, block_time DESC)").execute(pool).await?;

    // Create views - v_vendor_contracts_summary with extended fields
    sqlx::query(r#"
        CREATE OR REPLACE VIEW treasury.v_vendor_contracts_summary AS
        SELECT
            vc.id,
            vc.treasury_id,
            vc.project_id,
            vc.other_identifiers,
            vc.project_name,
            vc.description,
            vc.vendor_name,
            vc.vendor_address,
            vc.contract_url,
            vc.contract_address,
            vc.fund_tx_hash,
            vc.fund_slot,
            vc.fund_block_time,
            vc.initial_amount_lovelace,
            vc.status,
            vc.created_at,
            vc.updated_at,
            tc.contract_instance as treasury_instance,
            tc.name as treasury_name,
            COUNT(DISTINCT m.id) as total_milestones,
            COUNT(DISTINCT m.id) FILTER (WHERE m.status = 'pending') as pending_milestones,
            COUNT(DISTINCT m.id) FILTER (WHERE m.status = 'completed') as completed_milestones,
            COUNT(DISTINCT m.id) FILTER (WHERE m.status = 'disbursed') as disbursed_milestones,
            COALESCE(SUM(DISTINCT m.disburse_amount), 0)::BIGINT as total_disbursed_lovelace,
            COALESCE(SUM(u.lovelace_amount) FILTER (WHERE NOT u.spent), 0)::BIGINT as current_balance_lovelace,
            COUNT(u.id) FILTER (WHERE NOT u.spent) as utxo_count,
            (SELECT MAX(e.block_time) FROM treasury.events e WHERE e.vendor_contract_id = vc.id) as last_event_time,
            (SELECT COUNT(*) FROM treasury.events e WHERE e.vendor_contract_id = vc.id) as event_count
        FROM treasury.vendor_contracts vc
        LEFT JOIN treasury.treasury_contracts tc ON tc.id = vc.treasury_id
        LEFT JOIN treasury.milestones m ON m.vendor_contract_id = vc.id
        LEFT JOIN treasury.utxos u ON u.vendor_contract_id = vc.id
        GROUP BY vc.id, tc.contract_instance, tc.name
    "#).execute(pool).await?;

    sqlx::query(r#"
        CREATE OR REPLACE VIEW treasury.v_milestone_timeline AS
        SELECT
            m.id,
            m.milestone_id,
            m.milestone_order,
            m.label,
            m.description,
            m.acceptance_criteria,
            m.amount_lovelace,
            m.status,
            m.complete_tx_hash,
            m.complete_time,
            m.complete_description,
            m.evidence,
            m.disburse_tx_hash,
            m.disburse_time,
            m.disburse_amount,
            vc.project_id,
            vc.project_name,
            vc.vendor_address
        FROM treasury.milestones m
        JOIN treasury.vendor_contracts vc ON vc.id = m.vendor_contract_id
        ORDER BY vc.project_id, m.milestone_order
    "#).execute(pool).await?;

    sqlx::query(r#"
        CREATE OR REPLACE VIEW treasury.v_recent_events AS
        SELECT
            e.id,
            e.tx_hash,
            e.slot,
            e.block_number,
            e.block_time,
            e.event_type,
            e.amount_lovelace,
            e.reason,
            e.destination,
            e.metadata,
            e.created_at,
            tc.contract_instance as treasury_instance,
            vc.project_id,
            vc.project_name,
            m.label as milestone_label,
            m.milestone_order
        FROM treasury.events e
        LEFT JOIN treasury.treasury_contracts tc ON tc.id = e.treasury_id
        LEFT JOIN treasury.vendor_contracts vc ON vc.id = e.vendor_contract_id
        LEFT JOIN treasury.milestones m ON m.id = e.milestone_id
        ORDER BY e.slot DESC
    "#).execute(pool).await?;

    // v_treasury_summary with extended fields
    sqlx::query(r#"
        CREATE OR REPLACE VIEW treasury.v_treasury_summary AS
        SELECT
            tc.id as treasury_id,
            tc.contract_instance,
            tc.contract_address,
            tc.stake_credential,
            tc.name,
            tc.status,
            tc.publish_tx_hash,
            tc.publish_time,
            tc.initialized_tx_hash,
            tc.initialized_at,
            tc.permissions,
            COUNT(DISTINCT vc.id) as vendor_contract_count,
            COUNT(DISTINCT vc.id) FILTER (WHERE vc.status = 'active') as active_contracts,
            COUNT(DISTINCT vc.id) FILTER (WHERE vc.status = 'completed') as completed_contracts,
            COUNT(DISTINCT vc.id) FILTER (WHERE vc.status = 'cancelled') as cancelled_contracts,
            COALESCE(SUM(u.lovelace_amount) FILTER (WHERE NOT u.spent AND u.address = tc.contract_address), 0)::BIGINT as treasury_balance,
            COUNT(u.id) FILTER (WHERE NOT u.spent AND u.address = tc.contract_address) as utxo_count,
            (SELECT COUNT(*) FROM treasury.events WHERE treasury_id = tc.id) as total_events,
            (SELECT MAX(block_time) FROM treasury.events WHERE treasury_id = tc.id) as last_event_time,
            tc.created_at,
            tc.updated_at
        FROM treasury.treasury_contracts tc
        LEFT JOIN treasury.vendor_contracts vc ON vc.treasury_id = tc.id
        LEFT JOIN treasury.utxos u ON u.address = tc.contract_address
        GROUP BY tc.id
    "#).execute(pool).await?;

    // v_events_with_context - events with full context
    sqlx::query(r#"
        CREATE OR REPLACE VIEW treasury.v_events_with_context AS
        SELECT
            e.id,
            e.tx_hash,
            e.slot,
            e.block_number,
            e.block_time,
            e.event_type,
            e.amount_lovelace,
            e.reason,
            e.destination,
            e.metadata,
            e.created_at,
            tc.contract_instance as treasury_instance,
            tc.name as treasury_name,
            vc.project_id,
            vc.project_name,
            vc.vendor_name,
            vc.contract_address as project_address,
            m.milestone_id,
            m.label as milestone_label,
            m.milestone_order
        FROM treasury.events e
        LEFT JOIN treasury.treasury_contracts tc ON tc.id = e.treasury_id
        LEFT JOIN treasury.vendor_contracts vc ON vc.id = e.vendor_contract_id
        LEFT JOIN treasury.milestones m ON m.id = e.milestone_id
    "#).execute(pool).await?;

    // v_financial_summary - allocated vs disbursed vs remaining
    sqlx::query(r#"
        CREATE OR REPLACE VIEW treasury.v_financial_summary AS
        SELECT
            tc.id as treasury_id,
            tc.contract_instance,
            tc.name as treasury_name,
            COALESCE(SUM(vc.initial_amount_lovelace), 0)::BIGINT as total_allocated_lovelace,
            COALESCE(SUM(m_totals.total_disbursed), 0)::BIGINT as total_disbursed_lovelace,
            (COALESCE(SUM(vc.initial_amount_lovelace), 0) - COALESCE(SUM(m_totals.total_disbursed), 0))::BIGINT as total_remaining_lovelace,
            COALESCE(SUM(u.lovelace_amount) FILTER (WHERE NOT u.spent AND u.address = tc.contract_address), 0)::BIGINT as treasury_balance_lovelace,
            COALESCE((
                SELECT SUM(u2.lovelace_amount)
                FROM treasury.utxos u2
                JOIN treasury.vendor_contracts vc2 ON vc2.id = u2.vendor_contract_id
                WHERE vc2.treasury_id = tc.id AND NOT u2.spent
            ), 0)::BIGINT as project_balance_lovelace,
            COUNT(DISTINCT vc.id) as project_count,
            COUNT(DISTINCT CASE WHEN vc.status = 'active' THEN vc.id END) as active_project_count
        FROM treasury.treasury_contracts tc
        LEFT JOIN treasury.vendor_contracts vc ON vc.treasury_id = tc.id
        LEFT JOIN (
            SELECT
                m.vendor_contract_id,
                SUM(COALESCE(m.disburse_amount, 0)) as total_disbursed
            FROM treasury.milestones m
            GROUP BY m.vendor_contract_id
        ) m_totals ON m_totals.vendor_contract_id = vc.id
        LEFT JOIN treasury.utxos u ON u.address = tc.contract_address
        GROUP BY tc.id
    "#).execute(pool).await?;

    tracing::info!("Administration schema initialized successfully");
    Ok(())
}
