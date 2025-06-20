# rcosmos-exporter

`rcosmos-exporter` is a Rust-based exporter for collecting and exposing metrics from various Cosmos SDK-based blockchains and related networks. It is designed to scrape blockchain data, process it, and serve it in a format compatible with Prometheus for monitoring and alerting.

## Quick Start

### Prerequisites

- Rust toolchain (`cargo`, `rustc`)
- Prometheus (for scraping metrics)
- Docker (optional, for containerized deployment)

### Development

- Format: `cargo fmt`
- Test: `cargo test`
- Build: `cargo build`
- Run: `cargo run -- --env test-envs/.env.<network>.<mainnet|testnet>`

See `Makefile.toml` for more tasks.

### Environment Variables

- `BLOCKCHAIN` – Name of the blockchain (e.g., `Babylon`, `CoreDao`, `Lombard`, `Mezo`, `Namada`, `Noble`, `Tendermint`)
- `MODE` – `network` or `node`
- `NETWORK` – Network name (e.g., `mainnet`, `testnet`)
- `PROMETHEUS_IP` – IP to bind the metrics server (default: `0.0.0.0`)
- `PROMETHEUS_PORT` – Port for metrics (default: `9100`)
- `BLOCK_WINDOW` – Number of blocks to track (default: `500`)
- `NODE_NAME`, `NODE_RPC_ENDPOINT`, `NODE_REST_ENDPOINT` – Required in `node` mode
- `VALIDATOR_ALERT_ADDRESSES` – (Optional) Comma-separated validator addresses for alerting

You can also use a `.env` file or specify one with `--env <path>`.

### Metrics Endpoint

Metrics are exposed at `http://<PROMETHEUS_IP>:<PROMETHEUS_PORT>` in Prometheus format.


## Contributing

Contributions are welcome! Please open issues or pull requests.

## Features

- **Multi-Blockchain Support:**  
  Supports Babylon, CoreDAO, Lombard, Mezo, Namada, Noble, and generic Tendermint-based chains.
- **Prometheus Metrics:**  
  Exposes a wide range of metrics about blocks, validators, proposals, upgrades, and more.
- **Flexible Modes:**  
  Can run in `network` or `node` mode, adapting to different monitoring needs.
- **Customizable via Environment:**  
  Configure blockchain, network, endpoints, and more using environment variables or `.env` files.
- **Docker & CI Ready:**  
  Includes a Dockerfile and GitHub Actions workflows for building, testing, and releasing.

## Metrics

The exporter exposes the following Prometheus metrics (labels omitted for brevity):

### Exporter Metrics

- `rcosmos_exporter_http_request` – HTTP requests handled by the exporter
- `rcosmos_exporter_task_run` – Task runs
- `rcosmos_exporter_task_error` – Task errors
- `rcosmos_exporter_heartbeat` – Heartbeat timestamp
- `rcosmos_exporter_version_info` – Exporter version/build info

### Tendermint Metrics

- `tendermint_current_block_height` – Current block height
- `tendermint_current_block_time` – Current block time
- `tendermint_validator_missed_blocks` – Blocks missed by validator
- `tendermint_validators` – Validators on the network
- `tendermint_validator_uptime` – Validator uptime over block window
- `tendermint_validator_proposed_blocks` – Blocks proposed by validator
- `tendermint_validator_voting_power` – Validator voting power
- `tendermint_validator_proposer_priority` – Validator proposer priority
- `tendermint_validator_tokens` – Number of tokens by validator
- `tendermint_validator_jailed` – Jailed status by validator
- `tendermint_upgrade_status` – Upgrade status (1 if upgrade in progress)
- `tendermint_proposals` – Proposals in voting period
- `tendermint_upgrade_plan` – Upgrade plan info
- `tendermint_node_id` – Node ID
- `tendermint_node_catching_up` – Node catching up status
- `tendermint_node_latest_block_height` – Node latest block height
- `tendermint_node_latest_block_time` – Node latest block time
- `tendermint_node_earliest_block_height` – Node earliest block height
- `tendermint_node_earliest_block_time` – Node earliest block time
- `tendermint_block_txs` – Number of transactions in block
- `tendermint_block_tx_size` – Average transaction size in block
- `tendermint_block_gas_wanted` – Block gas wanted
- `tendermint_block_gas_used` – Block gas used
- `tendermint_block_tx_gas_wanted` – Average gas wanted per tx
- `tendermint_block_tx_gas_used` – Average gas used per tx
- `tendermint_node_app_name` – Node app name
- `tendermint_node_app_version` – Node app version
- `tendermint_node_app_commit` – Node app commit
- `tendermint_node_cosmos_sdk_version` – Node Cosmos SDK version
- `tendermint_node_moniker` – Node moniker
- `tendermint_validator_slashes` – Number of validator slashes
- `tendermint_validator_delegator_share` – Delegator share on validator
- `tendermint_validator_delegations` – Number of delegations on validator
- `tendermint_validator_unbonding_delegations` – Number of unbonding delegations
- `tendermint_validator_rewards` – Validator rewards
- `tendermint_validator_commissions` – Validator commissions
- `tendermint_validator_commission_rate` – Validator commission rate
- `tendermint_validator_commission_max_rate` – Validator commission max rate
- `tendermint_validator_commission_max_rate_change` – Validator commission max change rate
- `tendermint_address_balance` – Balance of monitored addresses

### Babylon Metrics

- `babylon_current_epoch` – Current epoch
- `babylon_validator_missing_bls_vote` – Validators missing BLS vote

### CoreDAO Metrics

- `coredao_validators` – Validator status (1=active, 0=inactive)
- `coredao_validator_jailed` – Validator jailed status
- `coredao_validator_slash_count` – Number of times validator slashed
- `coredao_validator_slash_block` – Block height of last slash
- `coredao_validator_participation` – Percentage of expected blocks signed
- `coredao_validator_recent_activity` – Signed at least one block in last rotation
- `coredao_validator_recent_activity_block` – Most recent block checked for activity
- `coredao_validator_signed_blocks_total` – Total blocks signed
- `coredao_validator_uptime` – Historical uptime percentage

### Namada Metrics

- `namada_current_epoch` – Current epoch
- `namada_validator_missing_vote` – Validators missing vote
- `namada_block_gas_used` – Block gas used
- `namada_block_gas_wanted` – Block gas wanted
- `namada_current_block_height` – Current block height
- `namada_current_block_time` – Current block time (unix)
- `namada_validator_missed_blocks` – Validator missed blocks
- `namada_validator_uptime` – Validator uptime

### Lombard Metrics

- `lombard_latest_session_id` – Latest notary session ID
- `lombard_validator_signed_latest_session` – Validator signed in latest session

## Showcase: Node Mode as a Kubernetes Sidecar

You can run `rcosmos-exporter` in `node` mode as a sidecar container alongside your Cosmos node in Kubernetes. This setup allows the exporter to scrape node-specific metrics and expose them for Prometheus scraping.

Below is an example Kubernetes Pod spec snippet:

```yaml
apiVersion: v1
kind: Pod
metadata:
  name: cosmos-node-with-exporter
spec:
  containers:
    - name: cosmos-node
      image: <your-cosmos-node-image>
        # ... other node envs ...
      ports:
        - containerPort: 26657
        - containerPort: 1317
    - name: rcosmos-exporter
      image: <your-rcosmos-exporter-image>
      env:
        - name: BLOCKCHAIN
          value: "Tendermint"
        - name: MODE
          value: "node"
        - name: NETWORK
          value: "mainnet"
        - name: NODE_NAME
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
        - name: NODE_RPC_ENDPOINT
          value: "http://localhost:26657"
        - name: NODE_REST_ENDPOINT
          value: "http://localhost:1317"
        - name: PROMETHEUS_IP
          value: "0.0.0.0"
        - name: PROMETHEUS_PORT
          value: "9100"
      ports:
        - containerPort: 9100
```

This will expose the metrics endpoint at `http://<pod-ip>:9100` for Prometheus to scrape.

## Public Helm Chart

A public Helm chart is available for deploying `rcosmos-exporter` to Kubernetes, making installation and management easy.

You can find it here: [p2p-org/cosmos-helm-charts: cosmos-exporter](https://github.com/p2p-org/cosmos-helm-charts/tree/main/charts/cosmos-exporter)

This chart provides a production-ready, configurable deployment for Cosmos-based netowrk monitoring in Kubernetes environments.
