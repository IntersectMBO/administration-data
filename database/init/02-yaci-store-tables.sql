-- YACI Store Schema for PostgreSQL
-- Combined from YACI Store 2.0.0 migration files
-- This schema is run before the application starts to create all required tables
-- All statements are idempotent (safe to re-run)

-- =====================================================
-- Create yaci_store schema
-- =====================================================
CREATE SCHEMA IF NOT EXISTS yaci_store;

-- Set search path so all tables are created in yaci_store schema
SET search_path TO yaci_store;

-- =====================================================
-- Core Tables (from components/core)
-- =====================================================

CREATE TABLE IF NOT EXISTS cursor_
(
    id          integer not null,
    block_hash  varchar(64),
    slot        bigint,
    block_number bigint,
    era         int,
    prev_block_hash varchar(64),
    create_datetime  timestamp,
    update_datetime  timestamp,
    primary key (id, block_hash)
);

CREATE INDEX IF NOT EXISTS idx_cursor_id ON cursor_(id);
CREATE INDEX IF NOT EXISTS idx_cursor_slot ON cursor_(slot);
CREATE INDEX IF NOT EXISTS idx_cursor_block_number ON cursor_(block_number);
CREATE INDEX IF NOT EXISTS idx_cursor_block_hash ON cursor_(block_hash);

CREATE TABLE IF NOT EXISTS era
(
    era        int not null primary key,
    start_slot bigint not null,
    block     bigint not null,
    block_hash varchar(64) not null
);

-- =====================================================
-- Block Store Tables (from stores/blocks)
-- =====================================================

CREATE TABLE IF NOT EXISTS block
(
    hash               varchar(64) not null
        primary key,
    number             bigint,
    body_hash          varchar(64),
    body_size          integer,
    epoch              integer,
    total_output       numeric(38)  null,
    total_fees         bigint       null,
    block_time         bigint       null,
    era                smallint,
    issuer_vkey        varchar(64),
    leader_vrf         jsonb,
    nonce_vrf          jsonb,
    prev_hash          varchar(64),
    protocol_version   varchar(64),
    slot               bigint,
    vrf_result         jsonb,
    vrf_vkey           varchar(64),
    no_of_txs          integer,
    slot_leader        varchar(56),
    epoch_slot         integer,
    op_cert_hot_vkey   varchar(64) null,
    op_cert_seq_number bigint null,
    op_cert_kes_period bigint null,
    op_cert_sigma      varchar(256) null,
    create_datetime    timestamp,
    update_datetime    timestamp
);

CREATE INDEX IF NOT EXISTS idx_block_number ON block(number);
CREATE INDEX IF NOT EXISTS idx_block_epoch ON block(epoch);
CREATE INDEX IF NOT EXISTS idx_block_slot_leader ON block(slot_leader);
CREATE INDEX IF NOT EXISTS idx_block_slot ON block(slot);

CREATE TABLE IF NOT EXISTS rollback
(
    id                     bigint not null primary key generated always as identity,
    rollback_to_block_hash varchar(64),
    rollback_to_slot       bigint,
    current_block_hash     varchar(64),
    current_slot           bigint,
    current_block          bigint,
    create_datetime        timestamp,
    update_datetime        timestamp
);

CREATE TABLE IF NOT EXISTS block_cbor
(
    block_hash      varchar(64) not null primary key,
    cbor_data       bytea not null,
    cbor_size       integer,
    slot            bigint not null,
    create_datetime timestamp,
    update_datetime timestamp
);

CREATE INDEX IF NOT EXISTS idx_block_cbor_slot ON block_cbor(slot);

-- =====================================================
-- UTXO Store Tables (from stores/utxo)
-- =====================================================

CREATE TABLE IF NOT EXISTS address_utxo
(
    tx_hash               varchar(64) not null,
    output_index          smallint    not null,
    slot                  bigint,
    block_hash            varchar(64),
    epoch                 integer,
    lovelace_amount       bigint       null,
    amounts               jsonb,
    data_hash             varchar(64),
    inline_datum          text,
    owner_addr            varchar(500),
    owner_addr_full       text,
    owner_stake_addr      varchar(255),
    owner_payment_credential varchar(56),
    owner_stake_credential  varchar(56),
    script_ref            text,
    reference_script_hash varchar(56) null,
    is_collateral_return  boolean,
    block                 bigint,
    block_time            bigint,
    update_datetime       timestamp,
    primary key (output_index, tx_hash)
);

CREATE INDEX IF NOT EXISTS idx_address_utxo_slot ON address_utxo(slot);
CREATE INDEX IF NOT EXISTS idx_reference_script_hash ON address_utxo(reference_script_hash);

CREATE TABLE IF NOT EXISTS tx_input
(
    output_index          smallint      not null,
    tx_hash                     varchar(64) not null,
    spent_at_slot               bigint,
    spent_at_block              bigint,
    spent_at_block_hash         varchar(64),
    spent_block_time            bigint,
    spent_epoch                 integer,
    spent_tx_hash               varchar(64) null,
    primary key (output_index, tx_hash)
);

CREATE INDEX IF NOT EXISTS idx_tx_input_slot ON tx_input(spent_at_slot);
CREATE INDEX IF NOT EXISTS idx_tx_input_block ON tx_input(spent_at_block);

CREATE TABLE IF NOT EXISTS address
(
    id                 bigserial,
    address            varchar(500) unique not null,
    addr_full          text,
    payment_credential varchar(56),
    stake_address      varchar(255),
    stake_credential   varchar(56),
    slot               bigint,
    update_datetime    timestamp,
    primary key (id)
);

CREATE INDEX IF NOT EXISTS idx_address_stake_address ON address (stake_address);
CREATE INDEX IF NOT EXISTS idx_address_slot ON address (slot);

CREATE TABLE IF NOT EXISTS ptr_address
(
    address            varchar(500),
    stake_address      varchar(255),
    primary key (address)
);

-- =====================================================
-- Transaction Store Tables (from stores/transaction)
-- =====================================================

CREATE TABLE IF NOT EXISTS transaction
(
    tx_hash                 varchar(64) not null
        primary key,
    auxiliary_datahash      varchar(64),
    block_hash              varchar(64),
    collateral_inputs       jsonb,
    collateral_return       jsonb,
    fee                     bigint,
    inputs                  jsonb,
    invalid                 boolean,
    network_id              smallint,
    outputs                 jsonb,
    reference_inputs        jsonb,
    required_signers        jsonb,
    script_datahash         varchar(64),
    slot                    bigint,
    total_collateral        bigint,
    ttl                     bigint,
    validity_interval_start bigint,
    collateral_return_json  jsonb,
    tx_index                integer,
    treasury_donation       bigint,
    epoch                   integer,
    block                   bigint,
    block_time              bigint,
    update_datetime         timestamp
);

CREATE INDEX IF NOT EXISTS idx_transaction_slot ON transaction(slot);

CREATE TABLE IF NOT EXISTS transaction_witness
(
    tx_hash varchar(64) not null,
    idx   integer not null,
    pub_key varchar(128),
    signature varchar(128),
    pub_keyhash varchar(56),
    type varchar(40),
    additional_data jsonb,
    slot bigint,
    primary key (tx_hash, idx)
);

CREATE INDEX IF NOT EXISTS idx_transaction_witness_slot ON transaction_witness(slot);

CREATE TABLE IF NOT EXISTS withdrawal
(
    tx_hash         varchar(64),
    address         varchar(255),
    amount          numeric(38),
    epoch           integer,
    slot            bigint,
    block           bigint,
    block_time      bigint,
    update_datetime timestamp,
    primary key (address, tx_hash)
);

CREATE INDEX IF NOT EXISTS idx_withdrawal_slot ON withdrawal(slot);

CREATE TABLE IF NOT EXISTS invalid_transaction
(
    tx_hash         varchar(64) not null
        primary key,
    slot            bigint not null,
    block_hash      varchar(64),
    transaction     jsonb,
    create_datetime timestamp,
    update_datetime timestamp
);

CREATE INDEX IF NOT EXISTS idx_invalid_transaction_slot ON invalid_transaction(slot);

CREATE TABLE IF NOT EXISTS transaction_cbor
(
    tx_hash         varchar(64) not null primary key,
    cbor_data       bytea not null,
    cbor_size       integer,
    slot            bigint not null,
    create_datetime timestamp,
    update_datetime timestamp
);

CREATE INDEX IF NOT EXISTS idx_transaction_cbor_slot ON transaction_cbor(slot);

-- =====================================================
-- Metadata Store Tables (from stores/metadata)
-- =====================================================

CREATE TABLE IF NOT EXISTS transaction_metadata
(
    id                    uuid not null primary key,
    slot                  bigint,
    tx_hash               varchar(64) not null,
    label                 varchar(255),
    body                  text,
    cbor                  text,
    block                  bigint,
    block_time            bigint,
    update_datetime       timestamp
);

CREATE INDEX IF NOT EXISTS idx_txn_metadata_slot ON transaction_metadata(slot);
CREATE INDEX IF NOT EXISTS idx_txn_metadata_label ON transaction_metadata(label);
CREATE INDEX IF NOT EXISTS idx_txn_metadata_tx_hash ON transaction_metadata(tx_hash);

-- =====================================================
-- Schema initialization complete
-- =====================================================
