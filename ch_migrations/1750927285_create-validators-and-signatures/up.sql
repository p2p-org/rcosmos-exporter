CREATE TABLE validators (`chain_id` String, `address` String) ENGINE = MergeTree ORDER BY (chain_id, address) ;

CREATE TABLE validators_signatures (
    chain_id String,
    height UInt64,
    address String,
    timestamp DateTime,
    signed UInt8
) ENGINE = MergeTree
PARTITION BY (toYear (timestamp) * 100) + toISOWeek (timestamp)
ORDER BY (chain_id, address, height);