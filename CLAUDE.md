# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

OCR intelligent pre-review system built with Rust. Processes documents (PDF/images) using PaddleOCR and evaluates them against configurable business rules.

**Tech Stack**: Rust 1.70+, Axum 0.7, Tokio, SQLite/DM database (via Go gateway), OSS/local storage, NATS JetStream (distributed mode)

## Common Commands

```bash
# Build
cargo build                          # Debug build
cargo build --release                # Release build
cargo build --release --features "dm_go,monitoring"  # Full features

# Run
cargo run                            # Dev mode
./scripts/ocr-server.sh start       # Start service (port 8964)
./scripts/ocr-server.sh status      # Check status
./scripts/ocr-server.sh restart     # Restart
./target/release/ocr-server worker  # Start as worker node

# Test & Quality
cargo clippy                        # Lint check
cargo fmt                           # Format code
./scripts/quality-check.sh          # Full quality check

# Build Scripts
./scripts/build.sh --prod           # MUSL static build (universal)
./scripts/build.sh --prod-native    # glibc native build (performance)
./scripts/build.sh --prod -f monitoring,dm_go  # With features

# Health Check
curl http://localhost:8964/api/health
curl http://localhost:8964/api/health/details
```

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         HTTP Layer (Axum)                        │
│   /api/preview  /api/auth  /api/files  /api/monitoring          │
└──────────────────────────┬──────────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────────┐
│                      AppState (src/lib.rs)                       │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────────┐   │
│  │ database │ │ storage  │ │task_queue│ │ semaphores        │   │
│  │ (trait)  │ │ (trait)  │ │ (trait)  │ │ (OCR concurrency) │   │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └───────────────────┘   │
└───────┼────────────┼────────────┼───────────────────────────────┘
        │            │            │
┌───────▼────┐ ┌─────▼─────┐ ┌────▼────────┐
│ SQLite/DM  │ │ OSS/Local │ │ Local/NATS  │
│ + Failover │ │ + Failover│ │ JetStream   │
└────────────┘ └───────────┘ └─────────────┘
```

**Key Abstractions**:
- `Database` trait (`src/db/traits.rs`): 70+ methods for preview records, rules, monitoring
- `Storage` trait (`src/storage/traits.rs`): put/get/delete with presigned URLs
- `TaskQueue` trait (`src/util/task_queue.rs`): enqueue preview tasks

**Global State** (`src/lib.rs`):
- `CONFIG`: Lazy-loaded configuration
- `CLIENT`: HTTP client (reqwest)
- `OCR_SEMAPHORE`: Concurrency limiter (default 6 tasks)
- `OSS`: OpenDAL operator for object storage

## Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point: server/worker/health-check modes |
| `src/lib.rs` | Global state, AppState struct, initialization |
| `src/api/preview.rs` | Core preview submission & processing |
| `src/api/worker_proxy.rs` | Worker heartbeat & result handling |
| `src/db/traits.rs` | Database trait with all query signatures |
| `src/db/failover.rs` | Smart failover: DM → SQLite |
| `src/storage/failover.rs` | Smart failover: OSS → local |
| `src/util/task_queue.rs` | Task queue abstraction (Local/NATS) |
| `src/util/rules/` | Business rule engine (zen-engine) |

## Configuration

Main config: `config/config.yaml` (use template: `config/config.template.yaml`)

Key environment variables:
```bash
# Deployment
OCR_DEPLOYMENT_ROLE=standalone|master|worker
OCR_DISTRIBUTED_ENABLED=true|false
OCR_NATS_URL=nats://host:4222

# Database
DB_PASSWORD=xxx
DM_GATEWAY_URL=http://host:8080
DM_GATEWAY_API_KEY=xxx

# Storage
OSS_ACCESS_KEY=xxx
OSS_ACCESS_SECRET=xxx
OSS_BUCKET=xxx

# Proxy bypass (important for internal OSS)
NO_PROXY=localhost,127.0.0.1,<oss-domain>
```

## Deployment Modes

| Mode | Command | Description |
|------|---------|-------------|
| standalone | `./ocr-server` | Single instance, local queue |
| master | `./ocr-server` with distributed config | Accepts requests, distributes via NATS |
| worker | `./ocr-server worker` | Processes tasks from NATS queue |

## Key APIs

| Endpoint | Auth | Description |
|----------|------|-------------|
| `POST /api/preview` | Third-party | Submit preview request |
| `GET /api/preview/data/:id` | SSO/Monitor | Get preview result |
| `GET /api/rules/matters/:id` | SSO/Monitor | Get matter rules |
| `GET /api/health/details` | None | Detailed health check |
| `GET /api/queue/status` | Monitor | Task queue status |
| `GET /api/failover/status` | None | Failover status |

**Authentication Types**:
- SSO: Via `/api/sso/callback` flow
- Monitor: `?monitor_session_id=xxx` or `X-Monitor-Session-Id` header
- Third-party: Signature-based (`X-Access-Key`, `X-Timestamp`, `X-Signature`)

## Failover Behavior

**Database** (smart mode):
1. Try DM via Go gateway (port 8080)
2. On failure → auto-failover to SQLite
3. Data persisted in `runtime/data/ocr.db`

**Storage**:
1. Try OSS (OpenDAL)
2. On failure → auto-failover to local (`runtime/fallback/storage/`)

Check status: `GET /api/failover/status`

## Development Notes

- **Port**: 8964 (configurable)
- **Logs**: `runtime/logs/`
- **Preview output**: `preview/` directory
- **Concurrency**: 6 OCR tasks max (matches engine pool)
- **Panic logs**: `./panic.log`, `runtime/logs/panic-*.log`

## Binary Targets

```bash
cargo run --bin ocr-server          # Main server
cargo run --bin daily-report        # Generate daily report
cargo run --bin import_matter_rules # Import rule configs
```
