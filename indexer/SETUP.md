# YACI Store Indexer Setup

## Prerequisites

- Docker and Docker Compose installed
- PostgreSQL (or use the docker-compose postgres service)

## Download YACI Store JAR

The YACI Store JAR file needs to be downloaded manually:

1. Visit https://github.com/bloxbean/yaci-store/releases
2. Download the latest `yaci-store-all-*.jar` file (e.g., `yaci-store-all-2.0.0-beta5.jar`)
3. Place it in the `indexer/` directory

Alternatively, you can try the download script:
```bash
./download-jar.sh 2.0.0-beta5
```

Note: The script may fail if the release tag format is different. In that case, download manually.

## Configuration

Edit `application.properties` to configure:
- Database connection details
- Cardano network settings
- Sync start point (slot/block)
- Which stores to enable/disable

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
