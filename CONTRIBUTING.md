# Contributing

Pull requests are welcome.

## Scope

- Keep contributions focused on the deployable OCR API service.
- Prefer small, reviewable changes over large mixed refactors.
- Preserve the existing Rust + Axum structure and bundled OCR runtime layout.

## Development Workflow

1. Update or create configuration from [`config/config.template.yaml`](/Users/xiaopang/ocr-server-src/config/config.template.yaml).
2. Build locally with `cargo build --release` or [`scripts/build.sh`](/Users/xiaopang/ocr-server-src/scripts/build.sh).
3. Verify the service starts and `GET /api/health` responds successfully.
4. Include documentation updates when behavior or deployment steps change.

## Submission Notes

- Keep secrets out of the repository.
- Use environment variables for credentials and deployment-specific settings.
- If a change affects APIs, update [`docs/api.md`](/Users/xiaopang/ocr-server-src/docs/api.md) and the examples under [`examples/`](/Users/xiaopang/ocr-server-src/examples).
