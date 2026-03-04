# OCR Server (Archive)

Rust-based OCR pre-review service. This repository is now archived and is no longer actively maintained.

## 1. Architecture Overview

Core flow: receive preview request -> download source files -> OCR processing -> rule evaluation -> persist/return result.

- API layer: `src/api/` for routing, auth, and request orchestration.
- Processing layer: `src/util/` for OCR, rule engine, and shared processing utilities.
- Data layer: `src/db/` with SQLite (default) and DM support through Go Gateway.
- Storage layer: `src/storage/` with OSS and local fallback.
- Bootstrap layer: `src/server/` and `src/main.rs` for startup and runtime mode control.

Runtime modes:
- `standalone`: single-node mode (default).
- `master/worker`: distributed mode with NATS JetStream task distribution.

## 2. Build and Packaging

Only one build script is retained:
- `scripts/build.sh`

Common build commands:

```bash
# Local release build
cargo build --release

# Unified script (native build)
./scripts/build.sh --prod-native

# Unified script (musl static build)
./scripts/build.sh --prod

# Build and package release output
./scripts/build.sh -m release -t native -p

# Container-based build (run build.sh inside image)
docker run --rm -it -v "$(pwd)":/workspace -w /workspace rust:1.82 \
  bash -lc "./scripts/build.sh --prod-native"
```

Main output locations:
- `target/` (compiler output)
- `build/` (script-generated package output)

## 3. Deployment

### Single-node Deployment (recommended minimal setup)

1. Copy template config: `cp config/config.template.yaml config/config.yaml`
2. Update settings for your environment (database, OSS, concurrency, auth).
3. Override sensitive values with environment variables (for example `DB_PASSWORD`, `OSS_ACCESS_SECRET`, `DM_GATEWAY_API_KEY`).
4. Start the binary:

```bash
./target/release/ocr-server
```

Default port: `8964`  
Health endpoint: `GET /api/health`

### Distributed Deployment (optional)

- Enable `distributed.enabled=true` in config.
- Set role to `master` or `worker`.
- Configure NATS server address (for example `nats://host:4222`).

## 4. Retained Repository Scope

- Retained: core source code, config templates, OCR assets, build script.
- Removed: test pages/scripts, non-essential operation/helper scripts, and split documentation files.
