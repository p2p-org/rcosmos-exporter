-- Reset chain state to a specific height
-- Replace placeholders: 0glabs-16661 and REPLACE_TARGET_HEIGHT

-- 1) Check current max height (replace chain_id)
SELECT max(height) FROM validators_signatures WHERE chain_id = '0glabs-16661';

-- 2) Delete anything above the target height (replace chain_id and height)
ALTER TABLE validators_signatures
DELETE WHERE chain_id = '0glabs-16661' AND height > 20215828;

-- 3) Ensure a marker row exists at the target height (so last_processed_height = REPLACE_TARGET_HEIGHT)
INSERT INTO validators_signatures (chain_id, height, timestamp, address, signed)
SELECT '0glabs-16661', 20215828, now(), 'init', 0
WHERE NOT EXISTS (
    SELECT 1 FROM validators_signatures WHERE chain_id = '0glabs-16661' AND height = 20215828
);

-- 4) Optional: Clear recent uptime buckets for this chain
ALTER TABLE validator_uptime_buckets
DELETE WHERE chain_id = '0glabs-16661' AND bucket_start >= now() - INTERVAL 31 DAY;

-- 5) Verify after mutations finish
-- SELECT max(height) FROM validators_signatures WHERE chain_id = '0glabs-16661' FINAL;
