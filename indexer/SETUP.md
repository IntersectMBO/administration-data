# YACI Store Indexer Setup

## Prerequisites

- Docker and Docker Compose installed
- PostgreSQL (or use the docker-compose postgres service)

## Download YACI Store JAR

The YACI Store JAR file needs to be downloaded manually:

1. Visit https://github.com/bloxbean/yaci-store/releases
2. Download the latest `yaci-store-all-*.jar` file (e.g., `yaci-store-all-2.0.0.jar`)
3. Place it in the `indexer/` directory

Alternatively, you can try the download script:
```bash
./download-jar.sh 2.0.0
```

Note: The script may fail if the release tag format is different. In that case, download manually.

## Configuration

### Environment Variables

Create a `.env` file in the project root with the treasury instance to filter for:

```bash
TREASURY_INSTANCE=9e65e4ed7d6fd86fc4827d2b45da6d2c601fb920e8bfd794b8ecc619
```

This is used by the `treasury-filter.mvel` plugin script to filter metadata by instance.

### Application Properties

Edit `application.properties` to configure:
- Database connection details
- Cardano network settings
- Which stores to enable/disable

### Database Schema

YACI Store manages its own database schema and tables automatically via Flyway migrations on startup. No manual table creation is needed.

## Build and Test

1. Build the Docker image:
```bash
docker build -t administration-indexer ./indexer
```

2. Start PostgreSQL (if not using docker-compose):
```bash
docker-compose up -d postgres
```

3. Run the indexer:
```bash
docker run --rm --network administration-data_administration-network \
  -v $(pwd)/indexer/application.properties:/app/application.properties:ro \
  administration-indexer
```

Or use docker-compose:
```bash
docker-compose up indexer
```

## Verify

Check the logs to ensure the indexer is syncing:
```bash
docker logs administration-indexer -f
```

You should see logs indicating blocks are being processed.
