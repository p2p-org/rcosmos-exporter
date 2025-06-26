CREATE TABLE validator_uptime_1d (
    chain_id String,
    address String,
    total_blocks UInt64,
    missed UInt64
) ENGINE = SummingMergeTree
ORDER BY (chain_id, address);

CREATE TABLE validator_uptime_7d (
    chain_id String,
    address String,
    total_blocks UInt64,
    missed UInt64
) ENGINE = SummingMergeTree
ORDER BY (chain_id, address);

CREATE TABLE validator_uptime_15d (
    chain_id String,
    address String,
    total_blocks UInt64,
    missed UInt64
) ENGINE = SummingMergeTree
ORDER BY (chain_id, address);

CREATE TABLE validator_uptime_30d (
    chain_id String,
    address String,
    total_blocks UInt64,
    missed UInt64
) ENGINE = SummingMergeTree
ORDER BY (chain_id, address);

CREATE MATERIALIZED VIEW mv_validator_uptime_1d TO validator_uptime_1d (
    chain_id String,
    address String,
    total_blocks UInt64,
    missed UInt64
) AS
SELECT
    chain_id,
    address,
    countDistinct (height) AS total_blocks,
    sum(if (signed = 0, 1, 0)) AS missed
FROM validators_signatures
WHERE
    timestamp >= (now() - toIntervalDay (1))
GROUP BY
    chain_id,
    address;

CREATE MATERIALIZED VIEW mv_validator_uptime_7d TO validator_uptime_7d (
    chain_id String,
    address String,
    total_blocks UInt64,
    missed UInt64
) AS
SELECT
    chain_id,
    address,
    countDistinct (height) AS total_blocks,
    sum(if (signed = 0, 1, 0)) AS missed
FROM validators_signatures
WHERE
    timestamp >= (now() - toIntervalDay (7))
GROUP BY
    chain_id,
    address;

CREATE MATERIALIZED VIEW mv_validator_uptime_15d TO validator_uptime_15d (
    chain_id String,
    address String,
    total_blocks UInt64,
    missed UInt64
) AS
SELECT
    chain_id,
    address,
    countDistinct (height) AS total_blocks,
    sum(if (signed = 0, 1, 0)) AS missed
FROM validators_signatures
WHERE
    timestamp >= (now() - toIntervalDay (15))
GROUP BY
    chain_id,
    address;

CREATE MATERIALIZED VIEW mv_validator_uptime_30d TO validator_uptime_30d (
    chain_id String,
    address String,
    total_blocks UInt64,
    missed UInt64
) AS
SELECT
    chain_id,
    address,
    countDistinct (height) AS total_blocks,
    sum(if (signed = 0, 1, 0)) AS missed
FROM validators_signatures
WHERE
    timestamp >= (now() - toIntervalDay (30))
GROUP BY
    chain_id,
    address;