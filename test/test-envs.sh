#!/bin/bash
set -euo pipefail

# Always build the binary at the start
BIN_PATH="target/release/rcosmos-exporter"
echo "🔨 Building the binary with cargo build --release..."
cargo build --release
chmod +x $BIN_PATH

failed_tests=()
error_metric_failed=()
env_files_found=0

# If an env file is passed as an argument, only test that file
single_env_file=""
if [ $# -ge 1 ]; then
  single_env_file="$1"
  if [ ! -f "$single_env_file" ]; then
    echo "❌ Provided env file does not exist: $single_env_file"
    exit 1
  fi
fi

if [ ! -d "test/env" ]; then
  echo "❌ test/env directory not found!"
  exit 1
fi

# Run migrations before tests
if docker compose ls &>/dev/null; then
  echo "🚀 Running migrations with docker compose..."
  docker compose -f docker-compose.test.yaml --project-name rcosmos-exporter-test up -d
else
  echo "❌ docker compose not found! Please install Docker Compose."
  exit 1
fi

sleep 10

docker logs $(docker compose --project-name rcosmos-exporter-test ps -q clickhouse-migrate)

sleep 120

files_to_test=()
if [ -n "$single_env_file" ]; then
  files_to_test+=("$single_env_file")
else
  for f in test/env/*.yaml; do files_to_test+=("$f"); done
fi

for env_file in "${files_to_test[@]}"; do
  export CLICKHOUSE_URL=http://localhost:18123
  export CLICKHOUSE_DATABASE=default
  export CLICKHOUSE_USER=default
  export CLICKHOUSE_PASSWORD=mysecurepassword123
  export NODE_NAME=rcosmos-exporter-test
  if [ -f "$env_file" ]; then
    env_files_found=$((env_files_found + 1))
    echo "🧪 Testing with $env_file"

    # Copy the env file to a temp location, do not modify
    tmp_env_file=$(mktemp)
    cp "$env_file" "$tmp_env_file"

    err_log_file=$(mktemp)

    "$BIN_PATH" --config "$tmp_env_file" 2> "$err_log_file" &
    app_pid=$!

    echo "⏳ Waiting 1 minute for exporter to run..."
    sleep 60

    metrics_output_file=$(mktemp)

    if curl -s "http://localhost:9100/metrics" > "$metrics_output_file"; then
      echo "✅ $env_file - metrics endpoint responded"
      error_lines=$(grep '^rcosmos_exporter_error{' "$metrics_output_file" || true)
      if [ -z "$error_lines" ]; then
        echo "❌ $env_file - rcosmos_exporter_error metrics not found!"
        error_metric_failed+=("$env_file (not found)")
      else
        while IFS= read -r line; do
          value=$(echo "$line" | awk '{print $NF}')
          module=$(echo "$line" | sed -n 's/.*module="\([^\"]*\)".*/\1/p')
          if [ "$value" != "0" ]; then
            echo "❌ $env_file - rcosmos_exporter_error for module '$module' is $value (should be 0)"
            error_metric_failed+=("$env_file ($module: $value)")
          fi
        done <<< "$error_lines"
        if [ ${#error_metric_failed[@]} -eq 0 ]; then
          echo "✅ $env_file - all rcosmos_exporter_error metrics are 0"
        fi
      fi
    else
      echo "❌ $env_file - metrics endpoint FAILED"
      failed_tests+=("$env_file")
    fi

    rm -f "$metrics_output_file"

    kill $app_pid 2>/dev/null || true
    sleep 3
    if kill -0 $app_pid 2>/dev/null; then
      kill -9 $app_pid 2>/dev/null || true
      sleep 1
    fi

    # Always show the error log
    if [ -s "$err_log_file" ]; then
      echo "---- Exporter error log for $env_file ----"
      cat "$err_log_file"
      echo "------------------------------------------"
    fi
    # Only fail if panic is found in the error log
    if grep -q 'panicked at' "$err_log_file"; then
      echo "❌ $env_file - exporter panicked!"
      failed_tests+=("$env_file (exporter panicked)")
    fi

    rm -f "$tmp_env_file" "$err_log_file"
    sleep 2
  fi

done

exit_code=0
if [ $env_files_found -eq 0 ]; then
  echo "❌ No test env YAML files found in test/env directory!"
  exit_code=1
fi

if [ ${#failed_tests[@]} -ne 0 ]; then
  echo "💥 ${#failed_tests[@]} test(s) failed:"
  printf '  - %s\n' "${failed_tests[@]}"
  exit_code=1
fi

if [ ${#error_metric_failed[@]} -ne 0 ]; then
  echo "💥 ${#error_metric_failed[@]} test(s) had rcosmos_exporter_error != 0 or not found:"
  printf '  - %s\n' "${error_metric_failed[@]}"
  exit_code=1
fi

if [ -z "$exit_code" ] || [ $exit_code -eq 0 ]; then
  echo "🎉 All $env_files_found tests passed!"
  exit 0
fi

exit $exit_code
