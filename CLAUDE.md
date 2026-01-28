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
cargo run                            # Dev mode (standalone)
./scripts/ocr-server.sh start       # Start service (port 8964)
./target/release/ocr-server worker  # Start as worker node

# Test & Quality
cargo test --all --no-fail-fast     # Run all tests
cargo clippy                        # Lint check (see clippy.toml for rules)
cargo fmt --all                     # Format code
./scripts/quality-check.sh          # Full quality check (fmt + clippy + tests)
./scripts/quality-check.sh --fix    # Auto-format then check

# Build for Production
./scripts/build.sh --prod           # MUSL static build (portable)
./scripts/build.sh --prod-native    # glibc native build (faster)
./scripts/build.sh --prod -f monitoring,dm_go  # With all features

# Health Check
curl http://localhost:8964/api/health
curl http://localhost:8964/api/health/details
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `monitoring` | Enables system metrics, bcrypt auth, process monitoring |
| `dm_go` | Enables DM database via HTTP Go gateway |
| `production_crypto` | Enables production AES-GCM encryption |
| `testing` | Test utilities |

Default: `reqwest` (HTTP client for downloads)

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

**Core Traits** (all in `src/`):
- `db/traits.rs`: Database interface (70+ methods for records, rules, monitoring)
- `storage/traits.rs`: Storage interface (put/get/delete with presigned URLs)
- `util/task_queue.rs`: Task queue interface (Local or NATS-backed)

**Global State** (`src/lib.rs`):
- `CONFIG`: Lazy-loaded YAML configuration
- `CLIENT`: Shared reqwest HTTP client
- `OCR_SEMAPHORE`: Concurrency limiter (default 6 tasks)
- `OSS`: OpenDAL operator for object storage

## Processing Pipeline

Preview request flow: `API → Queue → Download → OCR → Rules → Report`

1. **API** (`src/api/preview.rs`): Receives preview request with materials list
2. **Queue** (`src/util/task_queue.rs`): Enqueues for async processing (local channel or NATS)
3. **Evaluator** (`src/util/zen/evaluation.rs`): Orchestrates the full pipeline
4. **Download** (`src/util/zen/downloader.rs`): Fetches attachments from URLs
5. **Processing** (`src/util/processing/`): Multi-stage pipeline with resource prediction
   - `multi_stage_controller.rs`: Manages concurrent stages (download/convert/OCR/upload)
   - `resource_predictor.rs`: Predicts memory needs per task
6. **OCR** (`ocr-conn/`): Wrapper around PaddleOCR-json binary
   - `ocr.rs`: Engine pool management, process spawning
   - `preprocess.rs`: Image preprocessing utilities
7. **Rules** (`src/util/rules/`): Business rule evaluation using zen-engine
   - `repository.rs`: Loads rules from database
   - `cache.rs`: 5-minute TTL rule cache
   - `executor.rs`: Entry point for rule evaluation
8. **Report**: HTML/PDF generation of results

## Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Entry point: server/worker/health-check modes |
| `src/lib.rs` | Global state, AppState struct, initialization |
| `src/api/preview.rs` | Core preview submission & processing |
| `src/api/worker_proxy.rs` | Worker heartbeat & result handling |
| `src/db/failover.rs` | Smart failover: DM → SQLite |
| `src/storage/failover.rs` | Smart failover: OSS → local |
| `src/util/zen/evaluation.rs` | Preview evaluator (orchestrates OCR + rules) |
| `src/util/rules/` | Business rule engine (zen-engine based) |
| `ocr-conn/src/ocr.rs` | PaddleOCR process pool management |

## Deployment Modes

| Mode | Command | Description |
|------|---------|-------------|
| standalone | `./ocr-server` | Single instance, local queue |
| master | `./ocr-server` (distributed config) | Accepts requests, distributes via NATS |
| worker | `./ocr-server worker` | Pulls tasks from NATS, processes OCR |

Environment variables for distributed mode:
```bash
OCR_DEPLOYMENT_ROLE=master|worker|standalone
OCR_DISTRIBUTED_ENABLED=true
OCR_NATS_URL=nats://host:4222
```

## Configuration

Main config: `config/config.yaml` (copy from `config/config.template.yaml`)

Key environment overrides:
```bash
DB_PASSWORD=xxx                     # Database password
DM_GATEWAY_URL=http://host:8080     # DM Go gateway URL
DM_GATEWAY_API_KEY=xxx              # Gateway API key
OSS_ACCESS_KEY=xxx                  # Alibaba Cloud OSS
OSS_ACCESS_SECRET=xxx
NO_PROXY=localhost,127.0.0.1,<oss>  # Bypass proxy for internal OSS
```

## Failover Behavior

**Database** (smart mode): DM gateway → SQLite fallback. Check: `GET /api/failover/status`

**Storage**: OSS → local (`runtime/fallback/storage/`). Auto-sync when recovered.

## Code Style

- `cargo fmt --all` required; 4-space indent
- Clippy must pass (see `clippy.toml`): avoid `unwrap`/`expect`/`panic!`
- Prefer `?` for error propagation, early returns
- Keep secrets in env vars, never in code

## Binary Targets

```bash
cargo run --bin ocr-server          # Main server
cargo run --bin daily-report        # Generate daily report
cargo run --bin import_matter_rules # Import rule configs from JSON
```
