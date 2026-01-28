# OCR Server

High-performance document OCR pre-review system built with Rust.

## Features

- Multi-format OCR (PDF, JPG, PNG) powered by PaddleOCR
- Configurable business rule engine
- Smart failover (Database: DM → SQLite, Storage: OSS → Local)
- Distributed mode with NATS JetStream
- Real-time monitoring dashboard

## Tech Stack

- **Backend**: Rust 1.70+, Axum 0.7, Tokio
- **OCR**: PaddleOCR
- **Database**: SQLite (default) / DM (via Go gateway)
- **Storage**: Alibaba Cloud OSS / Local filesystem
- **Queue**: NATS JetStream (distributed mode)

## Quick Start

```bash
# Build
cargo build --release

# Configure
cp config/config.template.yaml config/config.yaml
# Edit config/config.yaml with your settings

# Run
./scripts/ocr-server.sh start

# Health check
curl http://localhost:8964/api/health
```

## Project Structure

```
src/
├── api/          # HTTP routes
├── db/           # Database layer (SQLite + DM failover)
├── storage/      # Storage layer (OSS + local failover)
├── model/        # Data models
├── server/       # Server bootstrap
└── util/         # Utilities (OCR, rules, auth, etc.)

config/           # Configuration templates
scripts/          # Management scripts
static/           # Web frontend
```

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health` | GET | Health check |
| `/api/preview` | POST | Submit preview request |
| `/api/preview/data/{id}` | GET | Get preview result |
| `/api/queue/status` | GET | Queue status |
| `/api/failover/status` | GET | Failover status |

## Configuration

Copy `config/config.template.yaml` to `config/config.yaml` and configure:

- Database connection (SQLite or DM gateway)
- OSS storage credentials
- Authentication settings
- Concurrency limits

Use environment variables for sensitive data:
```bash
export DB_PASSWORD="xxx"
export OSS_ACCESS_KEY="xxx"
export OSS_ACCESS_SECRET="xxx"
```

## Build for Production

```bash
# Static build (MUSL, portable)
./scripts/build.sh --prod

# Native build (glibc, better performance)
./scripts/build.sh --prod-native
```

## Documentation

- [Build Guide](docs/BUILD.md)
- [Operations](docs/OPERATIONS.md)

## License

MIT
