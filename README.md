# OCR Server (Archive)

Rust OCR pre-review service. This repository is archived.

## Architecture
- Flow: request -> download -> OCR -> rule evaluation -> result.
- API: `src/api/`
- Processing: `src/util/`
- Data: `src/db/` (SQLite default, DM via Go gateway)
- Storage: `src/storage/` (OSS + local fallback)
- Runtime modes: `standalone` or `master/worker` (NATS)

## Build
Only retained build script: `scripts/build.sh`

```bash
cargo build --release
./scripts/build.sh --prod-native
./scripts/build.sh --prod
./scripts/build.sh -m release -t native -p
```

Build outputs:
- `target/`
- `build/`

## Deploy
```bash
cp config/config.template.yaml config/config.yaml
./target/release/ocr-server
```

- Default port: `8964`
- Health: `GET /api/health`
- Use env vars for secrets (`DB_PASSWORD`, `DM_GATEWAY_API_KEY`, `OSS_ACCESS_SECRET`)

## Scope
- Retained: core source, config templates, OCR assets, `scripts/build.sh`
- Removed: test pages/scripts and non-essential helper scripts/docs
