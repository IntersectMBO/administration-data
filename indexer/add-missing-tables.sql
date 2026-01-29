-- Add missing tables to yaci_store schema
SET search_path TO yaci_store;

drop table if exists rollback cascade;
create table rollback
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

drop table if exists block_cbor cascade;
create table block_cbor
(
    block_hash      varchar(64) not null primary key,
    cbor_data       bytea not null,
    cbor_size       integer,
    slot            bigint not null,
    create_datetime timestamp,
    update_datetime timestamp
);

CREATE INDEX IF NOT EXISTS idx_block_cbor_slot ON block_cbor(slot);

-- Verify tables were created
SELECT table_name FROM information_schema.tables WHERE table_schema = 'yaci_store' ORDER BY table_name;
