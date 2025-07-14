CREATE TABLE validator_uptime_buckets (
    chain_id String,
    address String,
    bucket_start DateTime,
    total_blocks AggregateFunction (count, UInt64),
    missed AggregateFunction (sum, UInt64)
) ENGINE = AggregatingMergeTree ()
PARTITION BY
    toYYYYMMDD (bucket_start)
ORDER BY (
        chain_id, address, bucket_start
    );

CREATE MATERIALIZED VIEW mv_validator_uptime_buckets TO validator_uptime_buckets AS
SELECT
    chain_id,
    address,
    toStartOfHour (timestamp) AS bucket_start,
    countState (height) AS total_blocks,
    sumState (toUInt64 (signed = 0)) AS missed
FROM validators_signatures
GROUP BY
    chain_id,
    address,
    bucket_start;