# rcosmos-exporter

![rcosmos-exporter](docs/rcosmos-exporter.jpg)

**rcosmos-exporter** is a high-performance Rust exporter for Cosmos SDK-based blockchains and related networks. It collects, processes, and exposes rich metrics for Prometheus, with out-of-the-box support for ClickHouse storage, Grafana dashboards, and flexible deployment via Docker Compose.

---

## Table of Contents

- [Features](#features)
- [Quick Start (Docker Compose)](#quick-start-docker-compose)
- [Configuration](#configuration)
- [Modes: Node vs Network](#modes-node-vs-network)
- [Chain ID Handling](#chain-id-handling)
- [Available Modules](#available-modules)
- [Metrics](#metrics)
- [Grafana Dashboards](#grafana-dashboards)
- [Contributing](#contributing)
- [License](#license)

---

## Features

- **Multi-Blockchain Support:** Babylon, CoreDAO, Lombard, Mezo, Namada, Noble, and generic Tendermint-based chains.
- **Prometheus Metrics:** Exposes detailed metrics for blocks, validators, proposals, upgrades, and more.
- **ClickHouse Integration:** Stores historical validator and block data for advanced analytics.
- **Grafana Dashboards:** Prebuilt dashboards for instant observability.
- **Flexible Modes:** Run in `node` or `network` mode for granular or holistic monitoring.
- **Automatic Chain ID Discovery:** Seamless chain_id fetching for CometBFT-based chains.
- **Modern Config:** All configuration via YAML files—no more legacy env var sprawl.
- **Production Ready:** Docker Compose stack with Prometheus, Grafana, and ClickHouse.

---

## Quick Start (Docker Compose)

1. **Clone the repository:**
   ```sh
   git clone https://github.com/your-org/rcosmos-exporter.git
   cd rcosmos-exporter
   ```

2. **Edit your configuration:**
   - Use the provided `config.yaml` in the root as a template, or see additional working examples in the `test-envs/` directory for various networks and scenarios.

3. **Start stack and run ClickHouse migrations:**
   - Use the Docker Compose `migrate` profile to run all database migrations before starting the stack:
   ```sh
   docker-compose --profile migrate up --build
   ```
   - This will set up all required tables and views in ClickHouse.
   This launches:
   - `rcosmos-exporter`
   - `prometheus`
   - `grafana`
   - `clickhouse`

4. **Access services:**
   - **Grafana:** [http://localhost:3000](http://localhost:3000) (default: admin/admin)
   - **Prometheus:** [http://localhost:9090](http://localhost:9090)
   - **ClickHouse:** [http://localhost:8123](http://localhost:8123)
   - **Exporter metrics:** [http://localhost:9100/metrics](http://localhost:9100/metrics)

---

## Configuration

All configuration is now handled via a YAML file. No environment variables are required for normal operation **unless you are using persistence with ClickHouse or running in node mode**.

### Environment Variables

- **For ClickHouse persistence**, you must set the following environment variables (see `docker-compose.yaml` for examples):
  - `CLICKHOUSE_URL` (e.g. `http://clickhouse-server:8123`)
  - `CLICKHOUSE_DATABASE` (e.g. `default`)
  - `CLICKHOUSE_USER` (e.g. `default`)
  - `CLICKHOUSE_PASSWORD` (e.g. `mysecurepassword123`)

  Working example:
  ```sh
  export CLICKHOUSE_URL=http://localhost:8123
  export CLICKHOUSE_DATABASE=default
  export CLICKHOUSE_USER=default
  export CLICKHOUSE_PASSWORD=mysecurepassword123
  ```

- **For node mode**, you must set:
  - `NODE_NAME` (a unique name for the node being monitored)

These are required for the exporter to connect to ClickHouse and to identify the node in node mode.

### Example: `config.yaml`

```yaml
general:
  network: babylon-mainnet
  chain_id: cometbft
  mode: network
  metrics:
    address: 0.0.0.0
    port: 9100
    path: /metrics
  alerting:
    validators:
      - 44C395A4A96C6D1A450ED33B5A8DDB359CEFED36
  nodes:
    rpc:
      - name: p2p
        url: https://rpc.bbn-1.babylon.tm.p2p.org
        healthEndpoint: /health
    lcd:
      - name: p2p
        url: https://api.bbn-1.babylon.tm.p2p.org
        healthEndpoint: /cosmos/base/node/v1beta1/status

node:
  tendermint:
    nodeInfo:
      enabled: true
      interval: 30
  cometbft:
    status:
      enabled: true
      interval: 30

network:
  cometbft:
    validators:
      enabled: true
      interval: 10
    block:
      enabled: true
      interval: 10
      window: 500
      tx:
        enabled: true
      uptime:
        persistence: true
      
  tendermint:
    bank:
      addresses:
        - bbn1x3w0zqxn7tyfpawnylulhpyr9ds4v8rzdefjga
        - bbn1hz8qfjz2fduf9d437ev9w97plzdy8as0rc6lsr
        - bbn13ewj3k8g7kuvc7k0wk9v3nzzse5tvhe3lngp5q
        - bbn1wy0tyl8djfccnmaw67sxzr3kgnnnpal62lzxpx
      enabled: true
      interval: 30
    distribution:
      enabled: true
      interval: 30
    gov:
      enabled: true
      interval: 30
    staking:
      enabled: true
      interval: 30
    slashing:
      enabled: true
      interval: 30
    upgrade:
      enabled: true
      interval: 60
  
  mezo:
    poa:
      enabled: false
      interval: 30
  
  babylon:
    bls:
      enabled: true
      interval: 30
  
  lombard:
    ledger:
      addresses: []
      enabled: false
      interval: 30
  
  namada:
    account:
      addresses: []
      enabled: false
      interval: 30
    pos:
      enabled: false
      interval: 30
  
  coredao:
    block:
      enabled: false
      interval: 30
      window: 500
    validator:
      enabled: false
      interval: 30
```


**To use another config:**  
```sh
cargo run -- --config path/to/your-config.yaml
```
or in Docker Compose, mount your config and set the command accordingly.

---

## Modes: Node vs Network

- **Node Mode:**  
  Monitors a single node, exposes node-specific metrics (e.g., block production, validator status for that node).
- **Network Mode:**  
  Monitors the entire network, aggregates metrics across all nodes, tracks global validator set, proposals, upgrades, etc.

Choose the mode in your YAML config under `general.mode`.

---

## Chain ID Handling

- **Automatic:**  
  If `chain_id` is set to `"cometbft"` in your config, the exporter will fetch the chain ID from the first available RPC node at startup.
- **Manual:**  
  Set `chain_id` to the desired value in your config to override auto-discovery.

---

## Available Modules

The exporter supports multiple blockchain modules, each with its own metrics:

- **Babylon**
- **CoreDAO**
- **Lombard**
- **Mezo**
- **Namada**
- **Tendermint** (generic)
- **CometBFT** (for chain_id auto-discovery and block/validator metrics)

Modules are loaded automatically based on your config.

---

## Metrics

Metrics are exposed at `/metrics` in Prometheus format.  
**Categories include:**

- **Exporter:** Uptime, version, task status, HTTP requests
- **Blocks:** Height, time, transaction stats
- **Validators:** Uptime, missed blocks, voting power, slashing, rewards, commissions, first-seen, etc.
- **Proposals & Upgrades:** Governance and upgrade status
- **Chain-specific:** Babylon, CoreDAO, Namada, Lombard, etc.

**Historical metrics** are stored in ClickHouse for long-term analysis.

---

## Grafana Dashboards

- Prebuilt dashboards are included for quick visualization.
- Connect Grafana to Prometheus and ClickHouse (see `docker-compose.yaml` for provisioning).
- Dashboards auto-import on first launch.

---

## Advanced: ClickHouse

- All validator signatures, uptimes, and first-seen data are stored in ClickHouse.
- You can run custom analytics and long-term queries directly in ClickHouse or via Grafana.

---

## Contributing

Contributions are welcome!  
Please open issues or pull requests for bug fixes, features, or documentation.

---

## License

MIT or Apache-2.0 (choose your license and update here).

---

**For more details, see the code and configuration examples in this repository.**
