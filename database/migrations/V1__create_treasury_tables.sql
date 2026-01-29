-- Custom tables for administration data tracking
-- These tables extend YACI Store's schema with treasury-specific data

-- Treasury transactions with parsed metadata
CREATE TABLE IF NOT EXISTS treasury_transactions (
    id BIGSERIAL PRIMARY KEY,
    tx_hash VARCHAR(64) NOT NULL UNIQUE,
    slot BIGINT NOT NULL,
    block_number BIGINT NOT NULL,
    block_time TIMESTAMP NOT NULL,
    action_type VARCHAR(50), -- Fund, Disburse, Reorganize, SweepTreasury, Pause, Resume, Modify, Withdraw
    amount_lovelace BIGINT,
    amount_ada NUMERIC(20, 6) GENERATED ALWAYS AS (amount_lovelace / 1000000.0) STORED,
    metadata JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for treasury_transactions
CREATE INDEX IF NOT EXISTS idx_treasury_tx_hash ON treasury_transactions(tx_hash);
CREATE INDEX IF NOT EXISTS idx_treasury_tx_slot ON treasury_transactions(slot);
CREATE INDEX IF NOT EXISTS idx_treasury_tx_action_type ON treasury_transactions(action_type);
CREATE INDEX IF NOT EXISTS idx_treasury_tx_block_time ON treasury_transactions(block_time);

-- Treasury UTXOs tracking
CREATE TABLE IF NOT EXISTS treasury_utxos (
    id BIGSERIAL PRIMARY KEY,
    tx_hash VARCHAR(64) NOT NULL,
    output_index INTEGER NOT NULL,
    owner_addr VARCHAR(255) NOT NULL,
    lovelace_amount BIGINT NOT NULL,
    slot BIGINT NOT NULL,
    spent_at_slot BIGINT,
    is_spent BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(tx_hash, output_index)
);

-- Indexes for treasury_utxos
CREATE INDEX IF NOT EXISTS idx_treasury_utxo_owner ON treasury_utxos(owner_addr);
CREATE INDEX IF NOT EXISTS idx_treasury_utxo_spent ON treasury_utxos(is_spent);
CREATE INDEX IF NOT EXISTS idx_treasury_utxo_slot ON treasury_utxos(slot);

-- Vendor contracts discovered from transactions
CREATE TABLE IF NOT EXISTS vendor_contracts (
    id BIGSERIAL PRIMARY KEY,
    contract_address VARCHAR(255) NOT NULL UNIQUE,
    vendor_name VARCHAR(255),
    project_name VARCHAR(255),
    project_code VARCHAR(100),
    treasury_contract_address VARCHAR(255), -- Parent treasury contract
    created_at_tx_hash VARCHAR(64),
    created_at_slot BIGINT,
    current_balance_lovelace BIGINT DEFAULT 0,
    status VARCHAR(50) DEFAULT 'active', -- active, paused, completed, cancelled
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for vendor_contracts
CREATE INDEX IF NOT EXISTS idx_vendor_contract_address ON vendor_contracts(contract_address);
CREATE INDEX IF NOT EXISTS idx_vendor_contract_treasury ON vendor_contracts(treasury_contract_address);
CREATE INDEX IF NOT EXISTS idx_vendor_contract_status ON vendor_contracts(status);

-- Fund flows tracking movements between contracts
CREATE TABLE IF NOT EXISTS fund_flows (
    id BIGSERIAL PRIMARY KEY,
    tx_hash VARCHAR(64) NOT NULL,
    slot BIGINT NOT NULL,
    block_time TIMESTAMP NOT NULL,
    source_address VARCHAR(255) NOT NULL,
    destination_address VARCHAR(255) NOT NULL,
    amount_lovelace BIGINT NOT NULL,
    amount_ada NUMERIC(20, 6) GENERATED ALWAYS AS (amount_lovelace / 1000000.0) STORED,
    flow_type VARCHAR(50), -- Fund, Disburse, Withdraw, Sweep, etc.
    metadata JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for fund_flows
CREATE INDEX IF NOT EXISTS idx_fund_flows_tx_hash ON fund_flows(tx_hash);
CREATE INDEX IF NOT EXISTS idx_fund_flows_source ON fund_flows(source_address);
CREATE INDEX IF NOT EXISTS idx_fund_flows_destination ON fund_flows(destination_address);
CREATE INDEX IF NOT EXISTS idx_fund_flows_type ON fund_flows(flow_type);
CREATE INDEX IF NOT EXISTS idx_fund_flows_slot ON fund_flows(slot);
CREATE INDEX IF NOT EXISTS idx_fund_flows_block_time ON fund_flows(block_time);

-- Function to update updated_at timestamp
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- Triggers for updated_at
CREATE TRIGGER update_treasury_transactions_updated_at BEFORE UPDATE ON treasury_transactions
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_treasury_utxos_updated_at BEFORE UPDATE ON treasury_utxos
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_vendor_contracts_updated_at BEFORE UPDATE ON vendor_contracts
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
