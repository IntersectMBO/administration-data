#!/bin/bash

# Local Development Script for Administration Data System
# This script helps start all services for local development

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Functions
print_info() {
    echo -e "${BLUE}ℹ${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

check_command() {
    if command -v "$1" &> /dev/null; then
        return 0
    else
        return 1
    fi
}

# Check prerequisites
print_info "Checking prerequisites..."

if ! check_command docker; then
    print_error "Docker is not installed. Please install Docker first."
    exit 1
fi
print_success "Docker is installed"

if ! check_command docker-compose; then
    print_error "docker-compose is not installed. Please install docker-compose first."
    exit 1
fi
print_success "docker-compose is installed"

# Check if YACI Store JAR exists (support both yaci-store.jar and yaci-store-all-*.jar)
JAR_FILE=$(find indexer -name "yaci-store*.jar" -type f 2>/dev/null | head -1)
if [ -z "$JAR_FILE" ]; then
    print_warning "YACI Store JAR file not found in indexer/ directory"
    print_info "To download:"
    print_info "  Visit https://github.com/bloxbean/yaci-store/releases"
    print_info "  or use /indexer/download-jar.sh"
    INDEXER_AVAILABLE=false
else
    print_success "YACI Store JAR found: $JAR_FILE"
    INDEXER_AVAILABLE=true
fi

# Check if Rust is installed
if check_command cargo && check_command rustc; then
    RUST_VERSION=$(rustc --version 2>/dev/null | cut -d' ' -f2 || echo "unknown")
    if [ "$RUST_VERSION" != "unknown" ]; then
        print_success "Rust is installed ($RUST_VERSION)"
        API_AVAILABLE=true
    else
        print_warning "Rust is not properly configured. API will run via Docker only."
        print_info "To install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        API_AVAILABLE=false
    fi
else
    print_warning "Rust is not installed. API will run via Docker only."
    print_info "To install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    API_AVAILABLE=false
fi

# Parse command line arguments
COMMAND=${1:-"start"}

case "$COMMAND" in
    start)
        print_info "Starting local development environment..."
        
        # Start PostgreSQL
        print_info "Starting PostgreSQL..."
        docker-compose up -d postgres
        
        # Wait for PostgreSQL to be ready
        print_info "Waiting for PostgreSQL to be ready..."
        timeout=30
        counter=0
        while ! docker-compose exec -T postgres pg_isready -U postgres > /dev/null 2>&1; do
            sleep 1
            counter=$((counter + 1))
            if [ $counter -ge $timeout ]; then
                print_error "PostgreSQL failed to start within $timeout seconds"
                exit 1
            fi
        done
        
        # Wait a bit more for database to be fully initialized
        sleep 3
        
        # Verify database exists, create if it doesn't
        print_info "Verifying database exists..."
        MAX_RETRIES=10
        RETRY_COUNT=0
        DB_EXISTS=""
        
        while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
            # Try to check if database exists
            DB_EXISTS=$(docker-compose exec -T postgres psql -U postgres -tAc "SELECT 1 FROM pg_database WHERE datname='administration_data'" 2>/dev/null | tr -d '[:space:]' || echo "")
            if [ "$DB_EXISTS" = "1" ]; then
                print_success "Database 'administration_data' exists"
                break
            fi
            
            # If database doesn't exist, try to create it
            if [ $RETRY_COUNT -eq 0 ]; then
                print_info "Database 'administration_data' does not exist, creating..."
            fi
            
            CREATE_RESULT=$(docker-compose exec -T postgres psql -U postgres -c "CREATE DATABASE administration_data;" 2>&1)
            CREATE_EXIT_CODE=$?
            
            if [ $CREATE_EXIT_CODE -eq 0 ]; then
                print_success "Database 'administration_data' created"
                break
            elif echo "$CREATE_RESULT" | grep -q "already exists"; then
                print_success "Database 'administration_data' already exists"
                break
            else
                RETRY_COUNT=$((RETRY_COUNT + 1))
                if [ $RETRY_COUNT -lt $MAX_RETRIES ]; then
                    sleep 1
                fi
            fi
        done
        
        if [ "$DB_EXISTS" != "1" ] && [ $CREATE_EXIT_CODE -ne 0 ] && ! echo "$CREATE_RESULT" | grep -q "already exists"; then
            print_error "Failed to create database after $MAX_RETRIES attempts"
            print_error "Last error: $CREATE_RESULT"
            print_info "You may need to manually create the database or remove the postgres volume"
            print_info "To remove volume: docker-compose down -v"
        fi
        
        print_success "PostgreSQL is ready"

        # Ensure treasury schema exists (YACI Store creates its own schema/tables via Flyway)
        print_info "Ensuring treasury schema exists..."
        docker-compose exec -T postgres psql -U postgres -d administration_data \
            -f /docker-entrypoint-initdb.d/02-treasury-schema.sql 2>/dev/null || true
        print_success "Database schemas ready"

        # Start indexer if JAR is available
        if [ "$INDEXER_AVAILABLE" = true ]; then
            print_info "Starting indexer..."
            docker-compose up -d indexer
            print_success "Indexer started (check logs with: docker logs administration-indexer -f)"
        else
            print_warning "Skipping indexer (JAR file not found)"
        fi
        
        # Start API
        if [ "$API_AVAILABLE" = true ]; then
            print_info "Starting API (Rust)..."
            print_info "API will run in the foreground. Press Ctrl+C to stop."
            print_info "API will be available at http://localhost:8080"
            
            # Verify we can connect to the database from host before starting API
            print_info "Verifying database connection from host (port 5433)..."
            if ! PGPASSWORD=postgres psql -h localhost -p 5433 -U postgres -d administration_data -c "SELECT 1;" > /dev/null 2>&1; then
                print_warning "Cannot connect to database from host. This might be due to a local PostgreSQL on port 5432."
                print_info "Docker PostgreSQL is mapped to port 5433 to avoid conflicts."
                sleep 2
            fi
            
            cd api
            # Use port 5433 to connect to Docker PostgreSQL (avoids conflict with local PostgreSQL on 5432)
            DATABASE_URL="postgresql://postgres:postgres@localhost:5433/administration_data" cargo run
        else
            print_info "Starting API (Docker)..."
            docker-compose up -d api
            print_success "API started (check logs with: docker logs administration-api -f)"
            print_info "API is available at http://localhost:8080"
        fi
        ;;
    
    stop)
        print_info "Stopping all services..."
        docker-compose down
        print_success "All services stopped"
        ;;
    
    restart)
        print_info "Restarting services..."
        docker-compose restart
        print_success "Services restarted"
        ;;
    
    logs)
        SERVICE=${2:-""}
        if [ -z "$SERVICE" ]; then
            print_info "Showing logs for all services..."
            docker-compose logs -f
        else
            print_info "Showing logs for $SERVICE..."
            docker-compose logs -f "$SERVICE"
        fi
        ;;
    
    status)
        print_info "Service status:"
        docker-compose ps
        echo ""
        if [ "$INDEXER_AVAILABLE" = true ]; then
            print_success "Indexer: Ready (JAR found)"
        else
            print_warning "Indexer: Not ready (JAR missing)"
        fi
        if [ "$API_AVAILABLE" = true ]; then
            print_success "API: Ready (Rust installed)"
        else
            print_warning "API: Docker only (Rust not installed)"
        fi
        ;;
    
    build)
        print_info "Building Docker images..."
        
        if [ "$INDEXER_AVAILABLE" = true ]; then
            print_info "Building indexer..."
            docker-compose build indexer
            print_success "Indexer image built"
        else
            print_warning "Skipping indexer build (JAR file not found)"
        fi
        
        print_info "Building API..."
        docker-compose build api
        print_success "API image built"
        ;;
    
    clean)
        print_warning "This will stop and remove all containers, volumes, and networks"
        read -p "Are you sure? (y/N) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            print_info "Cleaning up..."
            docker-compose down -v
            print_success "Cleanup complete"
        else
            print_info "Cleanup cancelled"
        fi
        ;;
    
    help|--help|-h)
        echo "Administration Data System - Local Development Script"
        echo ""
        echo "Usage: ./dev.sh [command]"
        echo ""
        echo "Commands:"
        echo "  start     Start all services (default)"
        echo "  stop      Stop all services"
        echo "  restart   Restart all services"
        echo "  logs      Show logs (optionally specify service: postgres, indexer, api)"
        echo "  status    Show service status"
        echo "  build     Build Docker images"
        echo "  clean     Stop and remove all containers and volumes"
        echo "  help      Show this help message"
        echo ""
        echo "Examples:"
        echo "  ./dev.sh start              # Start all services"
        echo "  ./dev.sh logs indexer       # Show indexer logs"
        echo "  ./dev.sh status             # Check service status"
        ;;
    
    *)
        print_error "Unknown command: $COMMAND"
        echo "Run './dev.sh help' for usage information"
        exit 1
        ;;
esac
