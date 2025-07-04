#!/bin/bash

# Local environment setup script for rcosmos-exporter
# This script sets up Clickhouse, runs migrations, and optionally starts the application
#
# Usage:
#   ./local-env.sh up          # Start Clickhouse and run migrations
#   ./local-env.sh down        # Stop Clickhouse
#   ./local-env.sh status      # Show status of services
#   ./local-env.sh test        # Run tests (development mode with cargo run)
#   ./local-env.sh build-test  # Build application and run tests (production mode)
#   ./local-env.sh build-test --ci  # Use pre-built binary (for CI)
#   ./local-env.sh up --start-app  # Start Clickhouse, run migrations, and start the app

set -e

# Function to show usage
show_usage() {
    echo "Usage: $0 {up|down|status|test|build-test} [--start-app|--ci]"
    echo ""
    echo "Commands:"
    echo "  up          Start Clickhouse and run migrations"
    echo "  down        Stop Clickhouse"
    echo "  status      Show status of services"
    echo "  test        Run tests (development mode with cargo run)"
    echo "  build-test  Build application and run tests (production mode)"
    echo ""
    echo "Options:"
    echo "  --start-app Start the application after setup (only with 'up')"
    echo "  --ci        Use pre-built binary (for CI environments)"
    echo ""
    echo "Examples:"
    echo "  $0 up                    # Start Clickhouse and run migrations"
    echo "  $0 up --start-app        # Start Clickhouse, run migrations, and start app"
    echo "  $0 down                  # Stop Clickhouse"
    echo "  $0 status                # Show service status"
    echo "  $0 test                  # Run tests (development mode)"
    echo "  $0 build-test            # Build and run tests (production mode)"
    echo "  $0 build-test --ci       # Use pre-built binary (for CI)"
}

# Function to wait for Clickhouse to be ready
wait_for_clickhouse() {
    echo "🔍 Checking if Clickhouse is ready..."
    # Increase timeout in CI environments
    if [ "$CI_MODE" = true ]; then
        max_attempts=60  # 2 minutes in CI
        echo "⏱️ Using extended timeout for CI environment"
    else
        max_attempts=30  # 1 minute locally
    fi
    attempt=0
    while [ $attempt -lt $max_attempts ]; do
        if curl -s "http://localhost:8123/ping" > /dev/null; then
            echo "✅ Clickhouse is ready!"
            return 0
        fi
        echo "⏳ Waiting for Clickhouse... (attempt $((attempt + 1))/$max_attempts)"
        sleep 2
        attempt=$((attempt + 1))
    done

    echo "❌ Clickhouse failed to start within expected time"
    return 1
}

# Function to run migrations
run_migrations() {
    echo "🗄️ Running Clickhouse migrations..."
    for migration_dir in ch_migrations/*/; do
        if [ -f "${migration_dir}up.sql" ]; then
            echo "📝 Running migration: $(basename "$migration_dir")"
            # Use Docker to run clickhouse client from the container
            docker compose exec -T clickhouse-server clickhouse client \
                --host=localhost \
                --port=9000 \
                --user=default \
                --password='mysecurepassword123' \
                --database=default \
                --multiquery < "${migration_dir}up.sql"
        fi
    done
    echo "✅ Migrations completed!"
}

# Function to set environment variables
set_env_vars() {
    export CLICKHOUSE_URL=http://localhost:8123
    export CLICKHOUSE_DATABASE=default
    export CLICKHOUSE_USER=default
    export CLICKHOUSE_PASSWORD=mysecurepassword123

    echo "🔧 Environment variables set:"
    echo "  CLICKHOUSE_URL=$CLICKHOUSE_URL"
    echo "  CLICKHOUSE_DATABASE=$CLICKHOUSE_DATABASE"
    echo "  CLICKHOUSE_USER=$CLICKHOUSE_USER"
    echo "  CLICKHOUSE_PASSWORD=***"
}

# Function to start services
start_services() {
    echo "📦 Starting Clickhouse with Docker Compose..."

    # Add debugging for CI
    if [ "$CI_MODE" = true ]; then
        echo "🔍 CI Mode: Checking Docker status..."
        docker --version
        docker compose version
        docker ps -a
    fi

    docker compose up -d clickhouse-server clickhouse-ui

    # Add debugging for CI
    if [ "$CI_MODE" = true ]; then
        echo "🔍 CI Mode: Checking container status..."
        docker compose ps
        echo "🔍 CI Mode: Checking container logs..."
        docker compose logs clickhouse-server
    fi

    echo "⏳ Waiting for Clickhouse to be ready..."
    sleep 10

    if ! wait_for_clickhouse; then
        # Add debugging for CI
        if [ "$CI_MODE" = true ]; then
            echo "🔍 CI Mode: Debugging Clickhouse startup failure..."
            docker compose logs clickhouse-server
            docker compose ps
        fi
        exit 1
    fi

    run_migrations
    set_env_vars
}

# Function to stop services
stop_services() {
    echo "🛑 Stopping Clickhouse services..."
    docker compose down --remove-orphans --volumes
    echo "✅ Services stopped and volumes removed"
}

# Function to show status
show_status() {
    echo "📊 Service Status:"
    echo ""

    # Check if services are running
    if docker compose ps | grep -q "clickhouse-server"; then
        echo "✅ Clickhouse Server: Running"
        docker compose ps clickhouse-server
    else
        echo "❌ Clickhouse Server: Not running"
    fi

    echo ""
    if docker compose ps | grep -q "clickhouse-ui"; then
        echo "✅ Clickhouse UI: Running"
        docker compose ps clickhouse-ui
    else
        echo "❌ Clickhouse UI: Not running"
    fi

    echo ""
    echo "🔗 Useful URLs:"
    echo "  - Clickhouse UI: http://localhost:5521"
    echo "  - Clickhouse HTTP: http://localhost:8123"
    echo "  - Clickhouse Native: localhost:9000"
}

# Function to run tests (development mode)
run_tests() {
    echo "🧪 Running tests (development mode)..."

    # Ensure services are up
    if ! docker compose ps | grep -q "clickhouse-server"; then
        echo "📦 Starting services for testing..."
        start_services
    fi

    # Run cargo run for each test environment
    echo "🔍 Looking for test environment files..."

    if [ ! -d "test-envs" ]; then
        echo "❌ test-envs directory not found!"
        exit 1
    fi

            env_files_found=0
    failed_tests=()
    for env_file in test-envs/.env*; do
        if [ -f "$env_file" ]; then
            env_files_found=$((env_files_found + 1))
            echo "🧪 Testing with $env_file"

            # Set Clickhouse environment variables (matching CI)
            export CLICKHOUSE_URL=http://localhost:8123
            export CLICKHOUSE_DATABASE=default
            export CLICKHOUSE_USER=default
            export CLICKHOUSE_PASSWORD=mysecurepassword123

            # Run cargo run with the environment file
            env $(cat "$env_file" | xargs) cargo run -- --env "$env_file" &
            app_pid=$!

            # Wait for startup
            sleep 8

            # Check if metrics endpoint is responding (assuming default port 9100)
            if curl -s "http://localhost:9100/metrics" > /dev/null; then
                echo "✅ $env_file - SUCCESS"
            else
                echo "❌ $env_file - FAILED"
                failed_tests+=("$env_file")
            fi

            # Stop the application
            kill $app_pid 2>/dev/null || true
            sleep 2

            # Force kill if still running
            if kill -0 $app_pid 2>/dev/null; then
                echo "⚠️ Force killing $env_file (pid $app_pid)"
                kill -9 $app_pid 2>/dev/null || true
                sleep 1
            fi

            # Small delay between tests
            sleep 2
        fi
    done

    if [ $env_files_found -eq 0 ]; then
        echo "❌ No .env files found in test-envs directory!"
        exit 1
    fi

    # Report results (matching CI)
    if [ ${#failed_tests[@]} -eq 0 ]; then
        echo "🎉 All $env_files_found tests passed!"
    else
        echo "💥 ${#failed_tests[@]} out of $env_files_found test(s) failed:"
        printf '  - %s\n' "${failed_tests[@]}"
        exit 1
    fi
}

# Function to build and run tests (production mode)
build_and_test() {
    if [ "$CI_MODE" = true ]; then
        echo "🔨 Running tests with pre-built binary (CI mode)..."
    else
        echo "🔨 Building application and running tests (production mode)..."
    fi

    # Ensure services are up
    if ! docker compose ps | grep -q "clickhouse-server"; then
        echo "📦 Starting services for testing..."
        start_services
    fi

    # Build the application (skip if in CI mode)
    if [ "$CI_MODE" = false ]; then
        echo "🔨 Building application..."
        cargo build --release
    else
        echo "✅ Using pre-built binary from CI build job"
    fi

    # Run tests with built binary
    echo "🔍 Looking for test environment files..."

    if [ ! -d "test-envs" ]; then
        echo "❌ test-envs directory not found!"
        exit 1
    fi

            env_files_found=0
    failed_tests=()
    for env_file in test-envs/.env*; do
        if [ -f "$env_file" ]; then
            env_files_found=$((env_files_found + 1))
            echo "🧪 Testing with $env_file"

            # Set Clickhouse environment variables (matching CI)
            export CLICKHOUSE_URL=http://localhost:8123
            export CLICKHOUSE_DATABASE=default
            export CLICKHOUSE_USER=default
            export CLICKHOUSE_PASSWORD=mysecurepassword123

            # Run the built binary with the environment file
            env $(cat "$env_file" | xargs) ./target/release/rcosmos-exporter --env "$env_file" &
            app_pid=$!

            # Wait for startup
            sleep 8

            # Check if metrics endpoint is responding (assuming default port 9100)
            if curl -s "http://localhost:9100/metrics" > /dev/null; then
                echo "✅ $env_file - SUCCESS"
            else
                echo "❌ $env_file - FAILED"
                failed_tests+=("$env_file")
            fi

            # Stop the application
            kill $app_pid 2>/dev/null || true
            sleep 2

            # Force kill if still running
            if kill -0 $app_pid 2>/dev/null; then
                echo "⚠️ Force killing $env_file (pid $app_pid)"
                kill -9 $app_pid 2>/dev/null || true
                sleep 1
            fi

            # Small delay between tests
            sleep 2
        fi
    done

    if [ $env_files_found -eq 0 ]; then
        echo "❌ No .env files found in test-envs directory!"
        exit 1
    fi

    # Report results (matching CI)
    if [ ${#failed_tests[@]} -eq 0 ]; then
        echo "🎉 All $env_files_found tests passed!"
    else
        echo "💥 ${#failed_tests[@]} out of $env_files_found test(s) failed:"
        printf '  - %s\n' "${failed_tests[@]}"
        exit 1
    fi
}



# Check if command is provided
if [ $# -eq 0 ]; then
    show_usage
    exit 1
fi

COMMAND=$1
shift

# Parse additional arguments
START_APP=false
CI_MODE=false
while [[ $# -gt 0 ]]; do
    case $1 in
        --start-app)
            START_APP=true
            shift
            ;;
        --ci)
            CI_MODE=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            show_usage
            exit 1
            ;;
    esac
done

# Execute command
case $COMMAND in
    "up")
        echo "🚀 Setting up local environment for rcosmos-exporter..."
        start_services

        if [ "$START_APP" = true ]; then
            echo "🚀 Starting rcosmos-exporter..."
            echo "💡 Use Ctrl+C to stop the application"

            # Start with a test environment file if available
            if [ -f "test-envs/.env.tendermint" ]; then
                echo "🧪 Starting with Tendermint test configuration..."
                env $(cat "test-envs/.env.tendermint" | xargs) PROMETHEUS_PORT=9100 ./target/release/rcosmos-exporter
            else
                echo "⚠️ No test environment file found. You'll need to set blockchain-specific environment variables."
                echo "💡 Example: BLOCKCHAIN=tendermint RPC_ENDPOINTS=... REST_ENDPOINTS=... ./target/release/rcosmos-exporter"
            fi
        else
            echo ""
            echo "🎉 Local environment setup complete!"
            echo ""
            echo "📋 Next steps:"
            echo "  1. Set your blockchain-specific environment variables"
            echo "  2. Run: cargo run --release"
            echo "  3. Or run: ./local-env.sh up --start-app"
            echo ""
            echo "🔗 Useful URLs:"
            echo "  - Clickhouse UI: http://localhost:5521"
            echo "  - Clickhouse HTTP: http://localhost:8123"
            echo "  - Clickhouse Native: localhost:9000"
        fi
        ;;
    "down")
        echo "🛑 Stopping local environment..."
        stop_services
        ;;
    "status")
        echo "📊 Checking service status..."
        show_status
        ;;
    "test")
        echo "🧪 Running tests (development mode)..."
        run_tests
        ;;
    "build-test")
        echo "🔨 Building application and running tests (production mode)..."
        build_and_test
        ;;
    *)
        echo "Unknown command: $COMMAND"
        show_usage
        exit 1
        ;;
esac
