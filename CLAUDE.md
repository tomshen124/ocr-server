# CLAUDE.md

Archived repository. Keep changes minimal and practical.

## What Matters
- Main service: `ocr-server`
- Main build script: `scripts/build.sh`
- Main config template: `config/config.template.yaml`

## Quick Commands
```bash
cargo build --release
./scripts/build.sh --prod-native
./target/release/ocr-server
```

## Focus
- Prefer small, safe edits.
- Keep all user-facing docs in concise English.
- Do not commit secrets.
