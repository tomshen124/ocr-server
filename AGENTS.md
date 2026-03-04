# Repository Notes (Archived)

This project is archived and no longer maintained.

## Scope
- Keep only core source, config templates, OCR assets, and `scripts/build.sh`.
- Test pages/scripts and non-essential helper scripts were removed.

## Build
- `cargo build --release`
- `./scripts/build.sh --prod-native`
- `./scripts/build.sh --prod`

## Run
- `./target/release/ocr-server`
- Default health endpoint: `GET /api/health`

## Security
- Keep secrets out of git.
- Use env vars (for example `DB_PASSWORD`, `DM_GATEWAY_API_KEY`, `OSS_ACCESS_SECRET`).
