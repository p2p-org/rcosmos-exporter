CREATE TABLE validator_first_seen (
    chain_id String,
    address String,
    first_seen AggregateFunction (min, DateTime)
) ENGINE = AggregatingMergeTree
ORDER BY (chain_id, address);

CREATE MATERIALIZED VIEW mv_validator_first_seen TO validator_first_seen (
    chain_id String,
    address String,
    first_seen AggregateFunction (min, DateTime)
) AS
SELECT
    chain_id,
    address,
    minState (timestamp) AS first_seen
FROM validators_signatures
GROUP BY
    chain_id,
    address;