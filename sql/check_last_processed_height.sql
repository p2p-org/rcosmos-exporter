SELECT
    chain_id,
    MAX(height) as last_processed_height,
    MAX(timestamp) as last_processed_time,
    COUNT(*) as total_signatures
FROM validators_signatures
WHERE chain_id = 'REPLACE_CHAIN_ID'
GROUP BY chain_id;
