-- YACI Store Required Tables
-- This script creates the essential tables needed for YACI Store to run
-- Run this if migrations don't work automatically

-- Cursor table (fixed: id is BIGINT)
CREATE TABLE IF NOT EXISTS yaci_store.cursor_ (
    id BIGINT PRIMARY KEY,
    slot BIGINT NOT NULL,
    block_number BIGINT NOT NULL,
    block_hash VARCHAR(64),
    prev_block_hash VARCHAR(64),
    era VARCHAR(50),
    create_datetime TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    update_datetime TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Block table (block_time is BIGINT, not TIMESTAMP)
CREATE TABLE IF NOT EXISTS yaci_store.block (
    hash VARCHAR(64) PRIMARY KEY,
    body_hash VARCHAR(64),
    body_size BIGINT,
    block_time BIGINT,  -- Unix timestamp, not TIMESTAMP
    create_datetime TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    epoch BIGINT,
    epoch_slot BIGINT,
    era VARCHAR(50),
    issuer_vkey VARCHAR(255),
    leader_vrf VARCHAR(255),
    no_of_txs INTEGER,
    nonce_vrf VARCHAR(255),
    number BIGINT,
    op_cert_hot_vkey VARCHAR(255),
    op_cert_seq_number BIGINT,
    op_cert_sigma VARCHAR(255),
    op_cert_kes_period INTEGER,
    prev_hash VARCHAR(64),
    protocol_version VARCHAR(50),
    slot BIGINT,
    slot_leader VARCHAR(255),
    total_fees BIGINT,
    total_output BIGINT,
    update_datetime TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    vrf_result VARCHAR(255),
    vrf_vkey VARCHAR(255)
);

CREATE INDEX IF NOT EXISTS idx_block_slot ON yaci_store.block(slot);
CREATE INDEX IF NOT EXISTS idx_block_number ON yaci_store.block(number);
CREATE INDEX IF NOT EXISTS idx_block_epoch ON yaci_store.block(epoch);

-- Transaction table
CREATE TABLE IF NOT EXISTS yaci_store.transaction (
    tx_hash VARCHAR(64) PRIMARY KEY,
    auxiliary_datahash VARCHAR(64),
    block_hash VARCHAR(64),
    block BIGINT,
    block_time BIGINT,
    collateral_inputs TEXT,
    collateral_return TEXT,
    collateral_return_json JSONB,
    epoch BIGINT,
    fee BIGINT,
    inputs TEXT,
    invalid BOOLEAN,
    network_id INTEGER,
    outputs TEXT,
    reference_inputs TEXT,
    required_signers TEXT,
    script_datahash VARCHAR(64),
    slot BIGINT,
    total_collateral BIGINT,
    treasury_donation BIGINT,
    ttl BIGINT,
    tx_index INTEGER,
    validity_interval_start BIGINT,
    update_datetime TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_transaction_slot ON yaci_store.transaction(slot);
CREATE INDEX IF NOT EXISTS idx_transaction_block ON yaci_store.transaction(block);
CREATE INDEX IF NOT EXISTS idx_transaction_block_hash ON yaci_store.transaction(block_hash);
CREATE INDEX IF NOT EXISTS idx_transaction_epoch ON yaci_store.transaction(epoch);

-- Era table
CREATE TABLE IF NOT EXISTS yaci_store.era (
    era INTEGER PRIMARY KEY,
    block BIGINT,
    block_hash VARCHAR(64),
    start_slot BIGINT
);

CREATE INDEX IF NOT EXISTS idx_era_start_slot ON yaci_store.era(start_slot);
