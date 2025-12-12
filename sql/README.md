## SQL Scripts for ClickHouse

Helper scripts for managing chain state in ClickHouse. **Replace placeholders before running:**

- `REPLACE_CHAIN_ID` → the chain ID you want to operate on
- `REPLACE_TARGET_HEIGHT` → the block height you want to reset to

### Files

- **`check_last_processed_height.sql`**
  - Checks the current last processed height for a chain
  - Shows: max height, last processed time, and total signature count
  - Edit `REPLACE_CHAIN_ID` before running

- **`reset_chain_state.sql`**
  - Resets a chain's state to a specific height
  - Deletes all data above the target height
  - Inserts a marker row at the target height
  - Optionally clears recent uptime buckets (last 31 days)
  - Edit `REPLACE_CHAIN_ID` and `REPLACE_TARGET_HEIGHT` before running
  - Includes a verification query (commented out) to check the result

### Usage Example

```bash
# Check current state
clickhouse-client --query "$(sed 's/REPLACE_CHAIN_ID/celestia-testnet/g' sql/check_last_processed_height.sql)"

# Reset to height 9180825
clickhouse-client --query "$(sed -e 's/REPLACE_CHAIN_ID/celestia-testnet/g' -e 's/REPLACE_TARGET_HEIGHT/9180825/g' sql/reset_chain_state.sql)"
```
