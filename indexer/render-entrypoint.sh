#!/bin/bash
set -e

echo "=== YACI Store Render Entrypoint ==="

# Convert Render's postgres:// URL to JDBC format
# Render provides: postgres://user:password@host:port/database
# Spring needs: jdbc:postgresql://host:port/database (with user/pass separate)

if [ -n "$SPRING_DATASOURCE_URL" ]; then
    # Remove the protocol prefix
    URL_WITHOUT_PROTOCOL=$(echo "$SPRING_DATASOURCE_URL" | sed 's|^postgres://||' | sed 's|^postgresql://||')

    # Extract user:password (everything before @)
    USER_PASS=$(echo "$URL_WITHOUT_PROTOCOL" | sed 's|@.*||')

    # Extract host:port/database (everything after @)
    HOST_PORT_DB=$(echo "$URL_WITHOUT_PROTOCOL" | sed 's|.*@||')

    # Build JDBC URL with yaci_store schema as default
    JDBC_URL="jdbc:postgresql://${HOST_PORT_DB}?currentSchema=yaci_store"

    # Extract username and password if not already set
    if [ -z "$SPRING_DATASOURCE_USERNAME" ]; then
        SPRING_DATASOURCE_USERNAME=$(echo "$USER_PASS" | sed 's|:.*||')
        export SPRING_DATASOURCE_USERNAME
    fi

    if [ -z "$SPRING_DATASOURCE_PASSWORD" ]; then
        SPRING_DATASOURCE_PASSWORD=$(echo "$USER_PASS" | sed 's|[^:]*:||')
        export SPRING_DATASOURCE_PASSWORD
    fi

    export SPRING_DATASOURCE_URL="$JDBC_URL"
    echo "Configured database: ${HOST_PORT_DB}"
    echo "Database user: ${SPRING_DATASOURCE_USERNAME}"

    # Parse host, port, and database name for psql
    # Handle both host:port/db and host/db formats
    if echo "$HOST_PORT_DB" | grep -q ':[0-9]'; then
        # Has explicit port (host:port/database)
        DB_HOST=$(echo "$HOST_PORT_DB" | sed 's|:.*||')
        DB_PORT_AND_NAME=$(echo "$HOST_PORT_DB" | sed 's|[^:]*:||')
        DB_PORT=$(echo "$DB_PORT_AND_NAME" | sed 's|/.*||')
        DB_NAME=$(echo "$DB_PORT_AND_NAME" | sed 's|[^/]*/||')
    else
        # No explicit port (host/database) - default to 5432
        DB_HOST=$(echo "$HOST_PORT_DB" | sed 's|/.*||')
        DB_PORT="5432"
        DB_NAME=$(echo "$HOST_PORT_DB" | sed 's|[^/]*/||')
    fi

    echo "Parsed connection: host=$DB_HOST port=$DB_PORT database=$DB_NAME"

    # Initialize database schema before starting app
    echo "Initializing database schema..."
    export PGPASSWORD="$SPRING_DATASOURCE_PASSWORD"

    if psql -h "$DB_HOST" -p "$DB_PORT" -U "$SPRING_DATASOURCE_USERNAME" -d "$DB_NAME" -f /app/init-schema.sql; then
        echo "Database schema initialized successfully"
    else
        echo "Warning: Schema initialization had errors (tables may already exist)"
    fi

    unset PGPASSWORD
fi

# Copy template to config
cp /app/config/application.properties.template /app/config/application.properties

# Start YACI Store
echo "Starting YACI Store..."
exec java $JAVA_OPTS -jar /app/yaci-store.jar --spring.config.location=file:/app/config/application.properties
