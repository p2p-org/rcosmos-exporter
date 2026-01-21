-- Check for gaps in validator_signatures data
-- Replace 'story-1' with your actual chain_id if different

-- 1) Get overall statistics
SELECT
    chain_id,
    MIN(height) as min_height,
    MAX(height) as max_height,
    COUNT(DISTINCT height) as distinct_heights,
    (MAX(height) - MIN(height) + 1) as expected_heights,
    (MAX(height) - MIN(height) + 1) - COUNT(DISTINCT height) as missing_heights,
    COUNT(*) as total_signatures,
    MIN(timestamp) as earliest_timestamp,
    MAX(timestamp) as latest_timestamp
FROM validators_signatures
WHERE chain_id = 'story-1'
GROUP BY chain_id;

-- 2) Find specific gaps (missing heights) - this query finds gaps up to 1000 blocks
-- For larger gaps, you may need to adjust the range
SELECT
    height as missing_height
FROM (
    SELECT
        number as height
    FROM numbers(
        (SELECT MIN(height) FROM validators_signatures WHERE chain_id = 'story-1'),
        (SELECT MAX(height) FROM validators_signatures WHERE chain_id = 'story-1')
    )
) AS all_heights
WHERE height NOT IN (
    SELECT DISTINCT height
    FROM validators_signatures
    WHERE chain_id = 'story-1'
)
ORDER BY missing_height
LIMIT 100;

-- 3) Check for consecutive missing blocks (gaps larger than 1)
-- This helps identify if blocks were skipped in chunks
WITH height_ranges AS (
    SELECT
        height,
        LAG(height) OVER (ORDER BY height) as prev_height,
        height - LAG(height) OVER (ORDER BY height) as gap_size
    FROM (
        SELECT DISTINCT height
        FROM validators_signatures
        WHERE chain_id = 'story-1'
        ORDER BY height
    )
)
SELECT
    prev_height + 1 as gap_start,
    height - 1 as gap_end,
    gap_size - 1 as missing_blocks
FROM height_ranges
WHERE gap_size > 1
ORDER BY gap_start
LIMIT 50;

-- 4) Check recent blocks (last 1000) for gaps
SELECT
    MIN(height) as min_height,
    MAX(height) as max_height,
    COUNT(DISTINCT height) as distinct_heights,
    (MAX(height) - MIN(height) + 1) as expected_heights,
    (MAX(height) - MIN(height) + 1) - COUNT(DISTINCT height) as missing_heights
FROM validators_signatures
WHERE chain_id = 'story-1'
  AND height >= (SELECT MAX(height) - 1000 FROM validators_signatures WHERE chain_id = 'story-1');

-- 5) Check if there are any duplicate heights (shouldn't happen, but good to verify)
SELECT
    height,
    COUNT(*) as signature_count,
    COUNT(DISTINCT address) as validator_count
FROM validators_signatures
WHERE chain_id = 'story-1'
GROUP BY height
HAVING COUNT(*) > 200  -- Adjust based on expected validator count
ORDER BY signature_count DESC
LIMIT 20;
