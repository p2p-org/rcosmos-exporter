# Tendermint Metrics Exporter

This project provides a Rust-based exporter for monitoring Tendermint blockchain node metrics, particularly focusing on updating signatures and validator voting power. The exporter utilizes Prometheus for gathering and exposing metrics, enabling effective monitoring and alerting of Tendermint node health and performance.

## Key Rust Concepts

### 1. Traits
Traits in Rust are similar to interfaces in other languages. They define a set of methods that types must implement to satisfy the trait. Traits provide a way to define shared behavior in an abstract way.

### 2. ARC (Atomic Reference Counting)
`Arc` is a thread-safe reference-counting pointer. It enables multiple ownership of data by managing a reference count, which keeps track of the number of references to a value in memory. `Arc` is commonly used in concurrent programming when threads need to share ownership of an object.

### 3. Tokio
Tokio is an asynchronous runtime for Rust, enabling the writing of asynchronous applications. It provides essential utilities such as event loops, tasks, and various utilities to work with asynchronous I/O.

### 4. `tokio::spawn`
`tokio::spawn` is used to create a new asynchronous task. It spawns a new asynchronous task on the Tokio runtime, allowing concurrent execution of code without blocking the current thread.

### 5. Box
`Box` is a smart pointer that provides heap allocation in Rust. It is often used to store values that are not known at compile time and when the size of the data is dynamic.

### 6. `Box<dyn StdError + Send + Sync>`
`Box<dyn StdError + Send + Sync>` is a trait object that is often used to represent any kind of error that implements the standard `Error` trait, and is also `Send` (can be transferred between threads) and `Sync` (can be safely shared between threads).

## Project Overview

This project involves updating signatures and validator voting power for a Tendermint blockchain node. The process involves:

1. **Updating Signatures**: Continuously fetching and updating signatures from the blockchain.
2. **Updating Validator Voting Power**: Fetching the latest validator voting power data.
3. **Merging Data**: After updating the voting power, merging the signatures with the voting power data is performed to extract relevant information, such as addresses from the signatures. A hash table is used to quickly retrieve certain fields, and then the relevant metrics are set.

## Metrics

The following Prometheus metrics are exposed by the Tendermint metrics exporter:

### General Tendermint Node Metrics

- **`tendermint_current_block_height`** (`IntGauge`):
  Represents the current block height of the Tendermint node.
  **Description**: This metric shows the height of the latest committed block in the blockchain.

- **`tendermint_current_block_time`** (`IntGauge`):
  Represents the current block time of the Tendermint node.
  **Description**: This metric indicates the timestamp of the latest committed block.

### Validator Metrics

- **`tendermint_my_validator_missed_blocks`** (`GaugeVec`):
  Tracks the number of blocks missed by my validator.
  **Labels**: `["address"]`
  **Description**: Indicates how many blocks were missed by a specific validator address.

- **`tendermint_validator_missed_blocks`** (`GaugeVec`):
  Tracks the number of blocks missed by other validators.
  **Labels**: `["address"]`
  **Description**: Similar to `tendermint_my_validator_missed_blocks`, but for all validators.

- **`tendermint_current_voting_power`** (`GaugeVec`):
  Shows the current voting power of the validators.
  **Labels**: `["address", "name", "pub_key"]`
  **Description**: Provides information about the voting power of each validator, which is critical for assessing the validator's influence on consensus.

### Exporter Metrics

- **`tendermint_exporter_length_signatures_total`** (`IntCounter`):
  Represents the total number of blocks processed by the exporter.
  **Description**: This metric helps monitor how many blocks have been processed for exporting signatures.

- **`tendermint_exporter_length_signature_vector`** (`IntGauge`):
  Shows the total number of blocks processed in the vector.
  **Description**: Useful for understanding the length of the vector holding signatures.

- **`tendermint_exporter_rpc_health_check_requests_total`** (`IntCounter`):
  Tracks the total number of RPC health check requests made by the exporter.
  **Description**: Useful for monitoring the health check frequency of the Tendermint RPC endpoints.

- **`tendermint_exporter_rpc_health_check_failures_total`** (`IntCounter`):
  Shows the total number of RPC health check failures.
  **Description**: Critical for identifying issues with Tendermint RPC endpoints.

## Configuration

The application requires configuration through a `.env` file. An example configuration is provided in the `./env-local` file. Below is a description of each configuration option:

- **`LOGGING_LEVEL`**: Specifies the logging level of the application (e.g., `INFO`, `DEBUG`, `ERROR`).
- **`PROMETHEUS_IP`**: The IP address on which the Prometheus metrics server will be exposed.
- **`PROMETHEUS_PORT`**: The port on which the Prometheus metrics server will be exposed.
- **`BLOCK_WINDOW`**: Specifies the block window size used for monitoring and metrics calculations.
- **`REST_ENDPOINTS`**: A comma-separated list of REST endpoints for fetching validator data.
- **`RPC_ENDPOINTS`**: A comma-separated list of RPC endpoints for fetching block data and performing health checks.
- **`VALIDATOR_ADDRESS`**: The address of the validator that the exporter will monitor for missed blocks and voting power.

Ensure that your `.env` file is configured correctly to reflect your Tendermint setup and monitoring requirements.

## Running the Exporter

1. Ensure you have Rust and Cargo installed.
2. Clone the repository and navigate to the project directory.
3. Copy `./env-local` to `.env` and update it with your configuration:
   ```bash
   cp env-local .env
   ```
