#!/bin/bash

# Script to completely destroy and redeploy the Docker Compose setup
# This removes all containers, volumes, networks, and data

# Don't exit on error - we want to continue cleanup even if some steps fail
set +e

echo "🧹 Destroying Docker Compose setup and all local data..."

# Stop and remove containers, networks, and volumes
echo "📦 Stopping containers..."
docker compose --profile ci --profile migrate down -v --remove-orphans 2>/dev/null || true

# Remove any remaining volumes (in case they weren't removed by down -v)
echo "🗑️  Removing volumes..."
# Get project name from docker-compose (defaults to directory name)
PROJECT_NAME=$(basename "$(pwd)" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9]//g')
# Remove volumes by name pattern (docker compose creates volumes with project prefix)
VOLUMES=$(docker volume ls -q | grep -E "(${PROJECT_NAME}|clickhouse|prometheus|grafana|rcosmos)" || true)
if [ -n "$VOLUMES" ]; then
    echo "$VOLUMES" | xargs docker volume rm 2>/dev/null || true
fi
# Also try removing by exact volume names from docker-compose.yaml (with and without project prefix)
docker volume rm "${PROJECT_NAME}_clickhouse_data" "${PROJECT_NAME}_clickhouse_configs" "${PROJECT_NAME}_prometheus_data" "${PROJECT_NAME}_grafana_data" 2>/dev/null || true
docker volume rm clickhouse_data clickhouse_configs prometheus_data grafana_data 2>/dev/null || true

# Remove any orphaned containers
echo "🧹 Cleaning up orphaned containers..."
# Try multiple filter approaches (some systems need separate filters)
CONTAINERS=$(docker ps -a --format "{{.Names}}" | grep -E "(rcosmos|clickhouse|prometheus|grafana)" || true)
if [ -n "$CONTAINERS" ]; then
    echo "$CONTAINERS" | xargs docker rm -f 2>/dev/null || true
fi

# Remove any orphaned networks
echo "🌐 Cleaning up networks..."
NETWORKS=$(docker network ls --format "{{.Name}}" | grep -E "(${PROJECT_NAME}|rcosmos|clickhouse|prometheus|grafana)" || true)
if [ -n "$NETWORKS" ]; then
    echo "$NETWORKS" | xargs docker network rm 2>/dev/null || true
fi

# Prune system (optional - removes unused resources)
echo "🧹 Pruning unused Docker resources..."
docker system prune -f

echo "✅ Cleanup complete!"
echo ""
echo "🚀 Redeploying with Docker Compose..."
echo ""

# Redeploy with the specified command
# Re-enable exit on error for the actual deployment
set -e

# Start ClickHouse first and wait for it to be healthy
echo "🔄 Starting ClickHouse server..."
docker compose --profile ci up -d clickhouse-server

# Wait for ClickHouse to be ready
echo "⏳ Waiting for ClickHouse to be ready..."
for i in {1..30}; do
    if curl -s http://localhost:8123/ping > /dev/null 2>&1; then
        echo "✅ ClickHouse is ready!"
        break
    fi
    echo "   Waiting... ($i/30)"
    sleep 2
done

# Run migrations
echo "🔄 Running ClickHouse migrations..."
docker compose --profile ci --profile migrate run --rm clickhouse-migrate

# Start all other services
echo "🚀 Starting all services..."
docker compose --profile ci up -d --build clickhouse-ui exporter prometheus grafana

echo ""
echo "✅ Redeployment complete!"
echo ""
echo "📊 Services are starting up. Check status with:"
echo "   docker compose ps"
echo ""
echo "📝 View logs with:"
echo "   docker compose logs -f"
