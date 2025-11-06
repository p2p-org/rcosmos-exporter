#!/bin/bash
set -euo pipefail

# Prepare binary
BIN_PATH="target/release/rcosmos-exporter"
if [ "${PREBUILT:-}" = "1" ]; then
  echo "📦 Using prebuilt binary at $BIN_PATH"
  if [ ! -f "$BIN_PATH" ]; then
    echo "❌ Prebuilt binary not found at $BIN_PATH"
    exit 1
  fi
  chmod +x "$BIN_PATH"
else
  echo "🔨 Building the binary with cargo build --release..."
  cargo build --release
  chmod +x "$BIN_PATH"
fi

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

# Track whether we've started ClickHouse for any env that needs persistence
clickhouse_started=0

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

    # Start ClickHouse/migrations only if this env requires persistence (once)
    needs_clickhouse=0
    if grep -qs "persistence:\s*true" "$env_file"; then
      needs_clickhouse=1
    fi
    if [ $needs_clickhouse -eq 1 ] && [ $clickhouse_started -eq 0 ]; then
      if docker compose ls &>/dev/null; then
        echo "🚀 Running migrations with docker compose (first time)..."
        docker compose -f docker-compose.test.yaml --project-name rcosmos-exporter-test up -d
        # Wait for migration container to finish
        mig_cid=$(docker compose --project-name rcosmos-exporter-test ps -q clickhouse-migrate)
        if [ -z "$mig_cid" ]; then
          echo "❌ Migration container not found"
          exit 1
        fi
        echo "⏳ Waiting for migrations to complete..."
        for i in {1..120}; do
          status=$(docker inspect -f '{{.State.Status}}' "$mig_cid" || true)
          if [ "$status" = "exited" ]; then
            exit_code=$(docker inspect -f '{{.State.ExitCode}}' "$mig_cid" || echo 1)
            if [ "$exit_code" != "0" ]; then
              echo "❌ Migrations failed (exit $exit_code). Logs:" && docker logs "$mig_cid" || true
              exit 1
            fi
            echo "✅ Migrations completed"
            break
          fi
          sleep 2
        done
        clickhouse_started=1
      else
        echo "❌ docker compose not found! Please install Docker Compose."
        exit 1
      fi
    fi

    err_log_file=$(mktemp)

    "$BIN_PATH" --config "$tmp_env_file" 2> "$err_log_file" &
    app_pid=$!

    echo "⏳ Waiting for exporter metrics endpoint to be ready..."
    ready=0
    for i in {1..120}; do
      if curl -sf "http://localhost:9100/metrics" > /dev/null; then
        ready=1
        break
      fi
      if ! kill -0 $app_pid 2>/dev/null; then
        echo "❌ Exporter process exited early"
        break
      fi
      sleep 1
    done
    if [ $ready -ne 1 ]; then
      echo "❌ Metrics endpoint did not become ready in time"
      failed_tests+=("$env_file (metrics timeout)")
      if [ -s "$err_log_file" ]; then
        echo "---- Exporter error log for $env_file ----"
        cat "$err_log_file"
        echo "------------------------------------------"
      fi
      kill $app_pid 2>/dev/null || true
      continue
    fi

    metrics_output_file=$(mktemp)

    if curl -s "http://localhost:9100/metrics" > "$metrics_output_file"; then
      echo "✅ $env_file - metrics endpoint responded"
      # Stabilize and validate error metrics twice
      sleep 5
      all_zero=1
      for pass in 1 2; do
        if ! curl -s "http://localhost:9100/metrics" > "$metrics_output_file"; then
          all_zero=0; break
        fi
        error_lines=$(grep '^rcosmos_exporter_error{' "$metrics_output_file" || true)
        if [ -z "$error_lines" ]; then
          all_zero=0; break
        fi
        while IFS= read -r line; do
          value=$(echo "$line" | awk '{print $NF}')
          if [ "$value" != "0" ]; then
            all_zero=0
          fi
        done <<< "$error_lines"
        [ $pass -eq 1 ] && sleep 5
      done
      if [ $all_zero -ne 1 ]; then
        echo "❌ $env_file - rcosmos_exporter_error had non-zero values or missing"
        error_metric_failed+=("$env_file (error metrics)")
      else
        echo "✅ $env_file - error metrics stable at 0"
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
