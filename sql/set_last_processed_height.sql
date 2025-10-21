INSERT INTO validators_signatures (chain_id, height, address, timestamp, signed)
SELECT
    'atlantic-2' as chain_id,
    206653227 as height,  -- Recent block height
    address,
    now() as timestamp,
    1 as signed
FROM validators
WHERE chain_id = 'atlantic-2'
LIMIT 1;  -- Just one record to set the "last processed" height
