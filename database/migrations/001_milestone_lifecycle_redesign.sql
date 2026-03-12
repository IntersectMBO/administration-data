-- Migration: Milestone Lifecycle Redesign
-- Replaces linear status field with independent boolean flags
-- and renames disburse -> withdraw (disburse is treasury-level, not milestone-level)

BEGIN;

-- Add new columns
ALTER TABLE treasury.milestones
    ADD COLUMN time_limit BIGINT,
    ADD COLUMN withdrawn BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN evidence_provided BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN archived BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN withdraw_tx_hash VARCHAR(64),
    ADD COLUMN withdraw_time BIGINT,
    ADD COLUMN withdraw_amount BIGINT,
    ADD COLUMN archived_by_tx_hash VARCHAR(64),
    ADD COLUMN archived_at BIGINT,
    ADD COLUMN superseded_by INT REFERENCES treasury.milestones(id);

-- Migrate: completed/disbursed → evidence_provided
UPDATE treasury.milestones
SET evidence_provided = TRUE
WHERE status IN ('completed', 'disbursed');

-- Migrate: disbursed → withdrawn (best-effort mapping of old incorrect model)
UPDATE treasury.milestones
SET withdrawn = TRUE,
    withdraw_tx_hash = disburse_tx_hash,
    withdraw_time = disburse_time,
    withdraw_amount = disburse_amount
WHERE status = 'disbursed';

-- Drop old columns
ALTER TABLE treasury.milestones
    DROP COLUMN status,
    DROP COLUMN disburse_tx_hash,
    DROP COLUMN disburse_time,
    DROP COLUMN disburse_amount;

-- Replace UNIQUE constraint with partial unique index
ALTER TABLE treasury.milestones
    DROP CONSTRAINT IF EXISTS milestones_vendor_contract_id_milestone_id_key;

CREATE UNIQUE INDEX idx_milestone_active_unique
    ON treasury.milestones(vendor_contract_id, milestone_id)
    WHERE NOT archived;

CREATE INDEX IF NOT EXISTS idx_milestone_not_archived
    ON treasury.milestones(vendor_contract_id) WHERE NOT archived;

-- Drop old status index
DROP INDEX IF EXISTS treasury.idx_milestone_status;

COMMIT;
