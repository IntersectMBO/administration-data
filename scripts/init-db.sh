#!/bin/bash

# Script to ensure database exists
# This can be run after PostgreSQL starts to ensure the database is created

set -e

CONTAINER_NAME=${1:-"administration-postgres"}
DB_NAME=${2:-"administration_data"}

echo "Checking if database '$DB_NAME' exists in container '$CONTAINER_NAME'..."

# Check if database exists
DB_EXISTS=$(docker exec "$CONTAINER_NAME" psql -U postgres -tAc "SELECT 1 FROM pg_database WHERE datname='$DB_NAME'" 2>/dev/null | tr -d '[:space:]' || echo "")

if [ "$DB_EXISTS" != "1" ]; then
    echo "Database '$DB_NAME' does not exist. Creating..."
    docker exec "$CONTAINER_NAME" psql -U postgres -c "CREATE DATABASE $DB_NAME;" 2>&1
    if [ $? -eq 0 ]; then
        echo "✓ Database '$DB_NAME' created successfully"
    else
        echo "✗ Failed to create database"
        exit 1
    fi
else
    echo "✓ Database '$DB_NAME' already exists"
fi
