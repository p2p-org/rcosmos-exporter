-- Reset chain state to a specific height
-- Replace placeholders: REPLACE_CHAIN_ID and REPLACE_TARGET_HEIGHT

-- 1) Check current max height (replace chain_id)
SELECT max(height) FROM validators_signatures WHERE chain_id = 'REPLACE_CHAIN_ID';

-- 2) Delete anything above the target height (replace chain_id and height)
ALTER TABLE validators_signatures
DELETE WHERE chain_id = 'REPLACE_CHAIN_ID' AND height > REPLACE_TARGET_HEIGHT;

-- 3) Ensure a marker row exists at the target height (so last_processed_height = REPLACE_TARGET_HEIGHT)
INSERT INTO validators_signatures (chain_id, height, timestamp, address, signed)
SELECT 'REPLACE_CHAIN_ID', REPLACE_TARGET_HEIGHT, now(), 'init', 0
WHERE NOT EXISTS (
    SELECT 1 FROM validators_signatures WHERE chain_id = 'REPLACE_CHAIN_ID' AND height = REPLACE_TARGET_HEIGHT
);

-- 4) Optional: Clear recent uptime buckets for this chain
ALTER TABLE validator_uptime_buckets
DELETE WHERE chain_id = 'REPLACE_CHAIN_ID' AND bucket_start >= now() - INTERVAL 31 DAY;

-- 5) Verify after mutations finish
-- SELECT max(height) FROM validators_signatures WHERE chain_id = 'REPLACE_CHAIN_ID' FINAL;
