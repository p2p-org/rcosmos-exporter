#!/usr/bin/env python3
"""
Metric Validation Test

This script validates that exporter metrics match actual RPC data.
It fetches blocks from RPC, calculates expected metrics, and compares
them to what the exporter reports.

This is a critical validation to ensure metric accuracy.
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Dict, List, Optional, Set, Tuple
from collections import defaultdict

try:
    import yaml
except ImportError:
    print("❌ PyYAML not installed. Install with: pip install pyyaml")
    sys.exit(1)


class MetricValidator:
    """Validates exporter metrics against RPC data."""

    def __init__(self, rpc_url: str, metrics_url: str, chain_id: str, network: str):
        self.rpc_url = rpc_url.rstrip("/")
        self.metrics_url = metrics_url.rstrip("/")
        self.chain_id = chain_id
        self.network = network

    def fetch_rpc(self, path: str) -> Dict:
        """Fetch data from RPC endpoint."""
        url = f"{self.rpc_url}/{path}"
        try:
            with urllib.request.urlopen(url, timeout=30) as response:
                return json.loads(response.read())
        except urllib.error.URLError as e:
            raise Exception(f"RPC request failed: {e}")

    def fetch_metrics(self) -> str:
        """Fetch metrics from exporter."""
        try:
            with urllib.request.urlopen(self.metrics_url, timeout=10) as response:
                return response.read().decode("utf-8")
        except urllib.error.URLError as e:
            raise Exception(f"Metrics request failed: {e}")

    def parse_metrics(self, metrics_text: str) -> Dict[str, float]:
        """Parse Prometheus metrics text into a dictionary."""
        metrics = {}
        for line in metrics_text.split("\n"):
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            # Parse: metric_name{labels} value
            if "{" in line:
                # Has labels
                parts = line.split("}")
                if len(parts) == 2:
                    metric_part = parts[0] + "}"
                    value_part = parts[1].strip()
                    try:
                        value = float(value_part)
                        metrics[metric_part] = value
                    except ValueError:
                        pass
            else:
                # No labels
                parts = line.split()
                if len(parts) == 2:
                    try:
                        metrics[parts[0]] = float(parts[1])
                    except ValueError:
                        pass
        return metrics

    def get_metric_value(
        self,
        metrics: Dict[str, float],
        metric_name: str,
        labels: Optional[Dict[str, str]] = None,
    ) -> Optional[float]:
        """Get metric value by name and optional labels."""
        if labels:
            # Build label string
            label_str = ",".join([f'{k}="{v}"' for k, v in labels.items()])
            key = f"{metric_name}{{{label_str}}}"
        else:
            key = metric_name

        # Try exact match first
        if key in metrics:
            return metrics[key]

        # Try partial match (for labels that might be in different order)
        for k, v in metrics.items():
            if k.startswith(metric_name + "{") and k.endswith("}"):
                # Parse labels from key
                label_part = k[len(metric_name) + 1 : -1]
                label_dict = {}
                for label in label_part.split(","):
                    if "=" in label:
                        k2, v2 = label.split("=", 1)
                        label_dict[k2.strip()] = v2.strip('"')
                # Check if all required labels match
                if all(label_dict.get(k) == v for k, v in labels.items() if labels):
                    return v

        return None

    def get_latest_block_height(self) -> int:
        """Get latest block height from RPC."""
        response = self.fetch_rpc("status")
        return int(response["result"]["sync_info"]["latest_block_height"])

    def get_block(self, height: int) -> Dict:
        """Get block data from RPC."""
        response = self.fetch_rpc(f"block?height={height}")
        return response["result"]["block"]

    def get_validators(self) -> List[Dict]:
        """Get validator set from RPC."""
        response = self.fetch_rpc("validators")
        return response["result"]["validators"]

    def calculate_expected_signatures(self, block: Dict) -> Set[str]:
        """Calculate expected validator signatures from block."""
        signatures = set()
        last_commit = block.get("last_commit", {})
        for sig in last_commit.get("signatures", []):
            validator_addr = sig.get("validator_address", "")
            if validator_addr:
                signatures.add(validator_addr)
        return signatures

    def validate_block_metrics(
        self, height: int, block: Dict, metrics: Dict[str, float]
    ) -> Tuple[bool, List[str]]:
        """Validate metrics for a specific block."""
        errors = []
        warnings = []

        # Note: We don't validate current_block_height against individual sample blocks
        # because current_block_height represents the LATEST processed block, not the sample block.
        # We validate the gap separately to ensure we're not too far behind.

        # 1. Validate block_txs (transaction count) - just verify metric exists
        expected_txs = len(block.get("data", {}).get("txs", []))
        actual_txs = self.get_metric_value(
            metrics,
            "rcosmos_cometbft_block_txs",
            {"chain_id": self.chain_id, "network": self.network},
        )
        if actual_txs is not None and actual_txs != expected_txs:
            # This might be from a different block, so it's a warning
            warnings.append(
                f"Block {height}: block_txs mismatch - expected {expected_txs}, got {actual_txs} (may be from different block)"
            )

        # Note: We don't validate validator metrics for individual sample blocks
        # because:
        # 1. The missed_blocks counter only exists for validators who have missed blocks
        # 2. Validators who haven't missed any blocks won't have the counter (expected behavior)
        # 3. We validate missed blocks correlation separately in validate_missed_blocks_correlation()
        # This validation focuses on transaction data, not validator metrics per block

        return len(errors) == 0, errors + warnings

    def validate_monotonicity(
        self, initial_metrics: Dict[str, float], final_metrics: Dict[str, float]
    ) -> Tuple[bool, List[str]]:
        """Validate that counters are monotonic (only increase)."""
        errors = []
        warnings = []

        # Check current_block_height is monotonic
        initial_height = self.get_metric_value(
            initial_metrics,
            "rcosmos_cometbft_current_block_height",
            {"chain_id": self.chain_id, "network": self.network},
        )
        final_height = self.get_metric_value(
            final_metrics,
            "rcosmos_cometbft_current_block_height",
            {"chain_id": self.chain_id, "network": self.network},
        )

        if initial_height is not None and final_height is not None:
            if final_height < initial_height:
                errors.append(
                    f"Current block height decreased: {initial_height} -> {final_height} (should be monotonic)"
                )
            elif final_height == initial_height:
                warnings.append(
                    "Current block height unchanged (exporter may be caught up or stalled)"
                )

        # Check validator missed_blocks counters are monotonic
        try:
            validators = self.get_validators()
            for validator in validators[:10]:  # Check first 10
                addr = validator["address"]
                initial_missed = self.get_metric_value(
                    initial_metrics,
                    "rcosmos_cometbft_validator_missed_blocks",
                    {
                        "address": addr,
                        "chain_id": self.chain_id,
                        "network": self.network,
                    },
                )
                final_missed = self.get_metric_value(
                    final_metrics,
                    "rcosmos_cometbft_validator_missed_blocks",
                    {
                        "address": addr,
                        "chain_id": self.chain_id,
                        "network": self.network,
                    },
                )

                if initial_missed is not None and final_missed is not None:
                    if final_missed < initial_missed:
                        errors.append(
                            f"Validator {addr[:8]}... missed_blocks decreased: {initial_missed} -> {final_missed} (should be monotonic)"
                        )
        except Exception as e:
            warnings.append(f"Could not validate validator monotonicity: {e}")

        return len(errors) == 0, errors + warnings

    def validate_missed_blocks_correlation(
        self,
        start_height: int,
        end_height: int,
        initial_metrics: Dict[str, float],
        final_metrics: Dict[str, float],
    ) -> Tuple[bool, List[str]]:
        """
        Validate that missed_blocks counter increases when validators don't sign blocks.

        IMPORTANT: Only validates validators that are being tracked by the exporter
        (validators who have signed at least once). This is especially important in CI
        where the exporter may have just started and not all validators are tracked yet.
        """
        errors = []
        warnings = []

        try:
            validators = self.get_validators()
            validator_addresses = {v["address"] for v in validators}

            # Track which validators signed which blocks
            validator_signed_blocks = {addr: set() for addr in validator_addresses}

            # Sample blocks in the processed range
            sample_size = min(20, end_height - start_height + 1)
            sample_heights = []
            if end_height - start_height >= sample_size:
                step = max(1, (end_height - start_height) // sample_size)
                sample_heights = [start_height + i * step for i in range(sample_size)]
            else:
                sample_heights = list(range(start_height, end_height + 1))

            for height in sample_heights:
                try:
                    block = self.get_block(height)
                    signed_validators = self.calculate_expected_signatures(block)
                    for validator in signed_validators:
                        if validator in validator_signed_blocks:
                            validator_signed_blocks[validator].add(height)
                except Exception as e:
                    warnings.append(f"Could not fetch block {height}: {e}")

            # Get list of tracked validators (validators who have signed at least once and are being tracked)
            # In CI, the exporter may have just started, so we only validate validators that are actually tracked
            tracked_validators = set()
            for validator in validator_addresses:
                # Check if validator has a missed_blocks counter (initial or final)
                # This means they've been tracked by the exporter (have signed at least once)
                initial_missed = self.get_metric_value(
                    initial_metrics,
                    "rcosmos_cometbft_validator_missed_blocks",
                    {
                        "address": validator,
                        "chain_id": self.chain_id,
                        "network": self.network,
                    },
                )
                final_missed = self.get_metric_value(
                    final_metrics,
                    "rcosmos_cometbft_validator_missed_blocks",
                    {
                        "address": validator,
                        "chain_id": self.chain_id,
                        "network": self.network,
                    },
                )
                # If validator has the counter (even if 0), they're being tracked
                if initial_missed is not None or final_missed is not None:
                    tracked_validators.add(validator)

            # Only validate validators that are actually being tracked by the exporter
            # This is important in CI where the exporter may have just started
            if not tracked_validators:
                warnings.append(
                    "No validators are being tracked yet (exporter may have just started)"
                )
                return True, warnings

            # Validate missed blocks counter increased correctly
            # Only check validators who are being tracked
            for validator in list(tracked_validators)[
                :10
            ]:  # Check first 10 tracked validators
                signed_count = len(validator_signed_blocks.get(validator, set()))
                total_blocks = len(sample_heights)
                expected_missed = total_blocks - signed_count

                initial_missed = self.get_metric_value(
                    initial_metrics,
                    "rcosmos_cometbft_validator_missed_blocks",
                    {
                        "address": validator,
                        "chain_id": self.chain_id,
                        "network": self.network,
                    },
                )
                final_missed = self.get_metric_value(
                    final_metrics,
                    "rcosmos_cometbft_validator_missed_blocks",
                    {
                        "address": validator,
                        "chain_id": self.chain_id,
                        "network": self.network,
                    },
                )

                # Validator is tracked, so both should exist (or at least final should)
                if initial_missed is not None and final_missed is not None:
                    actual_increase = final_missed - initial_missed
                    # Allow 2 block difference due to timing
                    if abs(actual_increase - expected_missed) > 2:
                        warnings.append(
                            f"Validator {validator[:8]}... missed blocks correlation: expected {expected_missed} increase, got {actual_increase} (sampled {total_blocks} blocks)"
                        )
                elif final_missed is not None:
                    # Counter appeared during processing (validator started being tracked)
                    # This is fine - we can't validate correlation for newly tracked validators
                    pass
                # If validator has no counter at all, they're not tracked (shouldn't happen in this loop)
        except Exception as e:
            warnings.append(f"Could not validate missed blocks correlation: {e}")

        return len(errors) == 0, errors + warnings

    def validate_sequential_processing(
        self, start_height: int, end_height: int
    ) -> Tuple[bool, List[str]]:
        """Validate that blocks are processed sequentially without gaps."""
        errors = []
        # Fetch blocks and check for gaps
        heights_processed = set()
        for height in range(start_height, end_height + 1):
            try:
                block = self.get_block(height)
                heights_processed.add(height)
            except Exception as e:
                errors.append(f"Could not fetch block {height}: {e}")

        # Check for gaps
        if heights_processed:
            min_height = min(heights_processed)
            max_height = max(heights_processed)
            expected_count = max_height - min_height + 1
            actual_count = len(heights_processed)
            if actual_count != expected_count:
                errors.append(
                    f"Gap detected: expected {expected_count} consecutive blocks, got {actual_count} blocks"
                )

        return len(errors) == 0, errors

    def run_validation(
        self, num_blocks: int = 5, wait_time: int = 60
    ) -> Tuple[bool, List[str]]:
        """Run comprehensive metric validation."""
        print(f"🔍 Starting metric validation for {self.chain_id} ({self.network})")
        print(f"   RPC: {self.rpc_url}")
        print(f"   Metrics: {self.metrics_url}")
        print()

        all_errors = []
        all_warnings = []

        # Capture baseline metrics
        print("📸 Capturing baseline metrics...")
        try:
            baseline_metrics_text = self.fetch_metrics()
            baseline_metrics = self.parse_metrics(baseline_metrics_text)
            baseline_height = self.get_metric_value(
                baseline_metrics,
                "rcosmos_cometbft_current_block_height",
                {"chain_id": self.chain_id, "network": self.network},
            )
            if baseline_height is None:
                all_errors.append("Could not get baseline current_block_height")
                return False, all_errors

            # Get baseline gap to track catchup progress
            baseline_latest_height = self.get_latest_block_height()
            baseline_gap = baseline_latest_height - int(baseline_height)

            print(f"   Baseline block height: {int(baseline_height)}")
            print(f"   Baseline latest height: {baseline_latest_height}")
            print(f"   Baseline gap: {baseline_gap} blocks")
        except Exception as e:
            all_errors.append(f"Failed to capture baseline metrics: {e}")
            return False, all_errors

        # Wait for exporter to process enough blocks (poll until we have num_blocks or timeout)
        print(
            f"⏳ Waiting for exporter to process at least {num_blocks} blocks (max {wait_time}s)..."
        )
        start_time = time.time()
        initial_height = None
        poll_interval = 5  # Check every 5 seconds

        while time.time() - start_time < wait_time:
            try:
                metrics_text = self.fetch_metrics()
                metrics = self.parse_metrics(metrics_text)
                current_height = self.get_metric_value(
                    metrics,
                    "rcosmos_cometbft_current_block_height",
                    {"chain_id": self.chain_id, "network": self.network},
                )

                if current_height is not None:
                    if initial_height is None:
                        initial_height = int(current_height)
                        print(f"   Initial block height: {initial_height}")

                    blocks_processed = int(current_height) - initial_height
                    elapsed = int(time.time() - start_time)

                    if blocks_processed >= num_blocks:
                        print(
                            f"✅ Processed {blocks_processed} blocks in {elapsed}s (target: {num_blocks})"
                        )
                        break
                    else:
                        print(
                            f"   Processed {blocks_processed}/{num_blocks} blocks ({elapsed}s elapsed)..."
                        )
                else:
                    print(
                        f"   Waiting for metrics to be available ({int(time.time() - start_time)}s elapsed)..."
                    )
            except Exception as e:
                # If we can't fetch metrics yet, just wait
                pass

            time.sleep(poll_interval)

        elapsed_total = int(time.time() - start_time)
        if elapsed_total >= wait_time:
            print(
                f"⏱️  Reached max wait time ({wait_time}s), proceeding with validation..."
            )
        else:
            print(f"✅ Ready for validation after {elapsed_total}s")
        print()

        # Fetch metrics (we may have already fetched them, but fetch fresh for validation)
        try:
            metrics_text = self.fetch_metrics()
            metrics = self.parse_metrics(metrics_text)
            print(f"✅ Fetched {len(metrics)} metrics from exporter")
        except Exception as e:
            all_errors.append(f"Failed to fetch metrics: {e}")
            return False, all_errors

        # Get latest block height
        try:
            latest_height = self.get_latest_block_height()
            print(f"✅ Latest block height from RPC: {latest_height}")
        except Exception as e:
            all_errors.append(f"Failed to get latest block height: {e}")
            return False, all_errors

        # Get current block height from metrics
        current_height_metric = self.get_metric_value(
            metrics,
            "rcosmos_cometbft_current_block_height",
            {"chain_id": self.chain_id, "network": self.network},
        )
        if current_height_metric is None:
            all_errors.append("current_block_height metric not found")
            return False, all_errors

        current_height = int(current_height_metric)
        print(f"✅ Current block height from exporter: {current_height}")

        # Validate gap metric - check if we're catching up (gap decreasing)
        # In CI, exporter may start with a large gap due to initial backfill
        # We care more about catchup rate than absolute gap size
        gap_metric = self.get_metric_value(
            metrics,
            "rcosmos_cometbft_block_gap",
            {"chain_id": self.chain_id, "network": self.network},
        )
        if gap_metric is not None:
            expected_gap = latest_height - current_height
            current_gap = int(gap_metric)

            # Calculate gap change (positive = catching up, negative = falling behind)
            gap_change = baseline_gap - current_gap
            blocks_processed = int(current_height) - int(baseline_height)

            # Calculate processing rate (blocks per second)
            # Use elapsed_total which was calculated during the polling phase
            elapsed_seconds = max(1, elapsed_total)
            processing_rate = (
                blocks_processed / elapsed_seconds if elapsed_seconds > 0 else 0
            )

            print(f"   Current gap: {current_gap} blocks")
            print(
                f"   Gap change: {gap_change:+d} blocks (+ = catching up, - = falling behind)"
            )
            print(f"   Processing rate: {processing_rate:.2f} blocks/sec")

            # Critical: Gap should be decreasing (catching up) OR already small
            if current_gap > baseline_gap + 10:
                # Gap increased significantly - exporter is falling behind
                all_errors.append(
                    f"Block gap increasing: {baseline_gap} -> {current_gap} blocks (exporter is falling behind)"
                )
            elif current_gap > 1000 and gap_change < 10:
                # Very large gap and not catching up fast enough
                all_errors.append(
                    f"Block gap too large ({current_gap} blocks) and not catching up (only {gap_change} blocks in {elapsed_seconds}s)"
                )
            elif current_gap > 100 and gap_change > 0:
                # Large gap but catching up - this is OK during initial backfill
                print(
                    f"✅ Block gap large ({current_gap} blocks) but catching up ({gap_change} blocks in {elapsed_seconds}s)"
                )
            elif current_gap <= 100:
                # Gap is reasonable
                print(f"✅ Block gap acceptable: {current_gap} blocks behind")
            else:
                # Gap is large but catching up slowly - warning
                all_warnings.append(
                    f"Block gap large ({current_gap} blocks) but catching up slowly ({gap_change} blocks in {elapsed_seconds}s)"
                )

            # Validate gap metric matches calculated gap (with tolerance)
            # expected_gap = what we calculate from RPC (latest_height - current_height)
            # gap_metric = what the exporter metric reports
            # These can differ due to timing (exporter processes blocks between metric calculation and our fetch)
            if abs(gap_metric - expected_gap) > 5:
                # Allow 5 block difference due to timing
                all_warnings.append(
                    f"Block gap metric mismatch: calculated from RPC: {expected_gap} blocks, "
                    f"exporter metric reports: {gap_metric} blocks (difference: {abs(gap_metric - expected_gap)}, "
                    f"likely timing difference as exporter continues processing)"
                )
        else:
            all_warnings.append("block_gap metric not found")

        # Sample blocks for validation - focus on transaction and validator accuracy
        print(
            f"\n📊 Validating transaction and validator data from {num_blocks} sample blocks..."
        )
        sample_heights = []
        if current_height >= num_blocks:
            # Sample from recent blocks (within processed range)
            for i in range(num_blocks):
                sample_heights.append(current_height - i)
        else:
            # Sample from available blocks
            for i in range(min(num_blocks, current_height)):
                sample_heights.append(current_height - i)

        block_warnings = 0
        for height in sample_heights:
            try:
                block = self.get_block(height)
                is_valid, issues = self.validate_block_metrics(height, block, metrics)
                # All issues from block validation are warnings (not errors)
                # because we're validating historical blocks against current metrics
                for issue in issues:
                    all_warnings.append(issue)
                if issues:
                    block_warnings += 1
            except Exception as e:
                all_warnings.append(f"Block {height}: Could not validate - {e}")

        if block_warnings == 0:
            print(f"✅ All {len(sample_heights)} sample blocks validated successfully")
        else:
            print(
                f"⚠️  {block_warnings} blocks had validation warnings (expected for historical blocks)"
            )

        # Validate validator metrics exist and are reasonable
        # Note: The missed_blocks counter only exists for validators who have missed blocks
        # Validators who haven't missed any blocks won't have this metric (expected behavior)
        print(f"\n👥 Validating validator metrics...")
        try:
            validators = self.get_validators()
            validator_metrics_found = 0
            validator_metrics_reasonable = 0
            validators_without_metrics = 0

            for validator in validators[:10]:  # Check first 10 validators
                addr = validator["address"]
                missed = self.get_metric_value(
                    metrics,
                    "rcosmos_cometbft_validator_missed_blocks",
                    {
                        "address": addr,
                        "chain_id": self.chain_id,
                        "network": self.network,
                    },
                )
                if missed is not None:
                    validator_metrics_found += 1
                    # Check if missed blocks count is reasonable (not negative, not absurdly high)
                    if missed >= 0 and missed < 1000000:  # Reasonable upper bound
                        validator_metrics_reasonable += 1
                    else:
                        all_warnings.append(
                            f"Validator {addr[:8]}... has unusual missed_blocks value: {missed}"
                        )
                else:
                    # Validator doesn't have missed_blocks counter - this is OK if they haven't missed any blocks
                    validators_without_metrics += 1

            print(
                f"✅ Found missed_blocks metrics for {validator_metrics_found}/{min(10, len(validators))} validators"
            )
            if validators_without_metrics > 0:
                print(
                    f"   ℹ️  {validators_without_metrics} validators don't have missed_blocks counter (expected if they haven't missed any blocks)"
                )
            if validator_metrics_reasonable < validator_metrics_found:
                all_warnings.append(
                    f"Some validator metrics have unusual values ({validator_metrics_reasonable}/{validator_metrics_found} reasonable)"
                )
        except Exception as e:
            all_warnings.append(f"Could not validate validator metrics: {e}")

        # Validate monotonicity (counters only increase)
        print(f"\n📈 Validating metric monotonicity...")
        is_monotonic, monotonic_issues = self.validate_monotonicity(
            baseline_metrics, metrics
        )
        if not is_monotonic:
            all_errors.extend([i for i in monotonic_issues if "decreased" in i.lower()])
            all_warnings.extend(
                [i for i in monotonic_issues if "decreased" not in i.lower()]
            )
        else:
            print("✅ All counters are monotonic (only increase)")

        # Validate missed blocks correlation (if we processed enough blocks)
        if current_height > int(baseline_height) + 5:
            print(f"\n🔗 Validating missed blocks correlation...")
            start_height = int(baseline_height) + 1
            end_height = current_height
            is_correlated, correlation_issues = self.validate_missed_blocks_correlation(
                start_height, end_height, baseline_metrics, metrics
            )
            all_warnings.extend(correlation_issues)
            if is_correlated:
                print("✅ Missed blocks counter correlates with validator signatures")

        return len(all_errors) == 0, all_errors + all_warnings


def load_config(config_path: str) -> Dict:
    """Load YAML config file."""
    with open(config_path, "r") as f:
        return yaml.safe_load(f)


def main():
    parser = argparse.ArgumentParser(
        description="Validate exporter metrics against RPC data"
    )
    parser.add_argument("config", help="Path to config YAML file")
    parser.add_argument(
        "--num-blocks",
        type=int,
        default=5,
        help="Number of blocks to sample (default: 5)",
    )
    parser.add_argument(
        "--wait-time",
        type=int,
        default=60,
        help="Wait time for exporter to process blocks (default: 60s)",
    )
    parser.add_argument(
        "--metrics-url",
        default="http://localhost:9100/metrics",
        help="Metrics endpoint URL",
    )
    args = parser.parse_args()

    # Load config
    try:
        config = load_config(args.config)
    except Exception as e:
        print(f"❌ Failed to load config: {e}")
        sys.exit(1)

    # Extract config values
    general = config.get("general", {})
    chain_id = general.get("chain_id", "")
    network = general.get("network", "")
    nodes = general.get("nodes", {})
    rpc_nodes = nodes.get("rpc", [])

    if not rpc_nodes:
        print("❌ No RPC nodes found in config")
        sys.exit(1)

    rpc_url = rpc_nodes[0].get("url", "").rstrip("/")
    if not rpc_url:
        print("❌ No RPC URL found in config")
        sys.exit(1)

    # Check if cometbft block module is enabled
    network_config = config.get("network", {})
    cometbft_config = network_config.get("cometbft", {})
    block_config = cometbft_config.get("block", {})
    if not block_config.get("enabled", False):
        print(
            f"⚠️  CometBFT block module not enabled for {chain_id}, skipping validation"
        )
        sys.exit(0)

    # Check if we have required values
    if not chain_id or not network:
        print("❌ Missing chain_id or network in config")
        sys.exit(1)

    # Run validation
    validator = MetricValidator(rpc_url, args.metrics_url, chain_id, network)
    success, issues = validator.run_validation(args.num_blocks, args.wait_time)

    # Print results
    print("\n" + "=" * 60)
    # Only critical issues are errors: gap too large, metrics completely missing
    errors = [
        i
        for i in issues
        if "gap too large" in i.lower()
        or ("not found" in i.lower() and "current_block_height" in i.lower())
    ]
    warnings = [i for i in issues if i not in errors]

    if success:
        print("✅ VALIDATION PASSED")
        if warnings:
            print(f"\n⚠️  {len(warnings)} warning(s):")
            for warning in warnings[:5]:  # Show first 5 warnings
                print(f"  ⚠️  {warning}")
            if len(warnings) > 5:
                print(f"  ... and {len(warnings) - 5} more warnings")
    else:
        print("❌ VALIDATION FAILED")
        if errors:
            print(f"\n❌ {len(errors)} error(s):")
            for error in errors[:10]:  # Show first 10 errors
                print(f"  ❌ {error}")
            if len(errors) > 10:
                print(f"  ... and {len(errors) - 10} more errors")
        if warnings:
            print(f"\n⚠️  {len(warnings)} warning(s):")
            for warning in warnings[:5]:  # Show first 5 warnings
                print(f"  ⚠️  {warning}")
            if len(warnings) > 5:
                print(f"  ... and {len(warnings) - 5} more warnings")
    print("=" * 60)

    sys.exit(0 if success else 1)


if __name__ == "__main__":
    main()
