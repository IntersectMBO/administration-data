# YACI Store Indexer

This directory contains the configuration for YACI Store, a Cardano blockchain indexer that syncs blockchain data into PostgreSQL.

## Overview

YACI Store connects to a Cardano node and indexes:
- Blocks
- Transactions
- UTXOs
- Transaction metadata
- And more (configurable)

## Quick Start

The indexer runs as part of the docker-compose setup:

```bash
# From project root
./dev.sh start

# Check indexer logs
./dev.sh logs indexer

# Check sync status
curl http://localhost:8081/api/v1/blocks/latest
```

## Configuration

All configuration is in `application.properties`.

### Network Settings

```properties
# Mainnet (default)
store.cardano.host=backbone.cardano.iog.io
store.cardano.port=3001
store.cardano.protocol-magic=764824073

# Preprod
# store.cardano.host=preprod-node.play.dev.cardano.org
# store.cardano.port=3001
# store.cardano.protocol-magic=1

# Preview
# store.cardano.host=preview-node.play.dev.cardano.org
# store.cardano.port=3001
# store.cardano.protocol-magic=2
```

### Sync Start Point

The sync start point is configured via environment variables in `.env`:

```bash
STORE_CARDANO_SYNC_START_SLOT=160964954
STORE_CARDANO_SYNC_START_BLOCKHASH=560c7537831007f9670d287b15a69ba18a322b1edc39c0c23ccab3c12ad77b9f
```

Remove these from `.env` to sync from genesis.

### Enabled Stores

Control what data is indexed:

```properties
# Enabled (required for administration data tracking)
store.blocks.enabled=true
store.transaction.enabled=true
store.utxo.enabled=true
store.metadata.enabled=true

# Disabled (saves resources)
store.assets.enabled=false
store.epoch.enabled=false
store.mir.enabled=false
store.script.enabled=false
store.staking.enabled=false
store.governance.enabled=false
```

### Performance Tuning

```properties
# Parallel processing
store.executor.enable-parallel-processing=true
store.executor.blocks-batch-size=100
store.executor.blocks-partition-size=10
store.executor.use-virtual-thread-for-batch-processing=true
store.executor.use-virtual-thread-for-event-processing=true

# Database batch settings
store.db.batch-size=1000
store.db.parallel-insert=true
spring.datasource.hikari.maximum-pool-size=30
```

## API Endpoints

YACI Store exposes a REST API on port 8081:

### Blocks

```bash
# Latest block
curl http://localhost:8081/api/v1/blocks/latest

# Block by number
curl http://localhost:8081/api/v1/blocks/12125945

# Block by hash
curl http://localhost:8081/api/v1/blocks/hash/abc123...
```

### Transactions

```bash
# Transaction by hash
curl http://localhost:8081/api/v1/txs/abc123...

# Transactions in a block
curl http://localhost:8081/api/v1/blocks/12125945/txs
```

### UTXOs

```bash
# UTXOs by address
curl http://localhost:8081/api/v1/addresses/addr1.../utxos
```

### Health

```bash
# Health check
curl http://localhost:8081/actuator/health

# Prometheus metrics
curl http://localhost:8081/actuator/prometheus
```

## Database Schema

YACI Store creates tables in the `yaci_store` schema:

| Table | Description |
|-------|-------------|
| `block` | Block headers and metadata |
| `transaction` | Transaction data with inputs/outputs as JSONB |
| `address_utxo` | UTXO set with multi-asset support |
| `transaction_metadata` | Transaction metadata by label |
| `cursor_` | Current sync position |
| `era` | Era transition markers |

### Example Queries

```sql
-- Latest blocks
SELECT number, slot, epoch, no_of_txs, TO_TIMESTAMP(block_time)
FROM yaci_store.block
ORDER BY number DESC LIMIT 10;

-- Transaction count by epoch
SELECT epoch, COUNT(*) as tx_count
FROM yaci_store.transaction
GROUP BY epoch
ORDER BY epoch DESC;

-- UTXOs for an address
SELECT tx_hash, output_index, amounts, slot
FROM yaci_store.address_utxo
WHERE owner_addr = 'addr1...'
AND spent = false;

-- Metadata by label
SELECT tx_hash, body, slot
FROM yaci_store.transaction_metadata
WHERE label = 674
ORDER BY slot DESC LIMIT 10;
```

## Docker Image

The indexer uses the official YACI Store Docker image:

```yaml
image: bloxbean/yaci-store:2.0.0
```

Configuration is mounted at `/app/config/application.properties`.

## Troubleshooting

### Check Sync Progress

```bash
# Current position
docker exec administration-postgres psql -U postgres -d administration_data \
  -c "SELECT * FROM yaci_store.cursor_;"

# Block count
docker exec administration-postgres psql -U postgres -d administration_data \
  -c "SELECT COUNT(*) FROM yaci_store.block;"
```

### Restart Indexer

```bash
docker-compose restart indexer
```

### Reset and Resync

```bash
# Stop services
./dev.sh stop

# Remove database volume
docker volume rm administration-data_postgres_data

# Start fresh
./dev.sh start
```

### Memory Issues

Increase JVM heap in docker-compose.yml:

```yaml
environment:
  - JAVA_OPTS=-Xmx4g -Xms2g
```

## Resources

- [YACI Store Documentation](https://store.yaci.xyz/)
- [YACI Store GitHub](https://github.com/bloxbean/yaci-store)
- [Configuration Reference](https://store.yaci.xyz/stores/configuration)
