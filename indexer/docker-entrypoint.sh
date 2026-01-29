#!/bin/sh

# Set JVM options for YACI Store
export JAVA_OPTS="${JAVA_OPTS:--Xmx2g -Xms1g}"

echo "Waiting for PostgreSQL to be ready..."

# Wait for PostgreSQL to be available (max 60 seconds)
MAX_RETRIES=30
RETRY_COUNT=0
while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
    if nc -z postgres 5432 2>/dev/null; then
        echo "PostgreSQL is available!"
        break
    fi
    RETRY_COUNT=$((RETRY_COUNT + 1))
    echo "Waiting for PostgreSQL... ($RETRY_COUNT/$MAX_RETRIES)"
    sleep 2
done

if [ $RETRY_COUNT -eq $MAX_RETRIES ]; then
    echo "ERROR: PostgreSQL not available after $MAX_RETRIES retries"
    exit 1
fi

# Give PostgreSQL a moment to fully initialize
sleep 3

echo "Starting YACI Store..."
exec java $JAVA_OPTS -jar /app/yaci-store.jar --spring.config.location=file:/app/application.properties
