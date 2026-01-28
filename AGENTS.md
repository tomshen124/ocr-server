# Repository Guidelines

## Project Structure & Modules
- `src/`: Rust server code. `api/` holds routes/controllers, `db/` wraps DM Go gateway + SQLite connectors, `storage/` handles OSS/local, `server/` boots HTTP, `model/` defines data types, `bin/` contains CLIs (`import_matter_rules`, `daily_report`, `requeue_queued`).
- `scripts/`: automation for build/deploy/test (`build.sh`, `build-with-docker.sh`, `quality-check.sh`, `ocr-server.sh`, `dm-gateway.sh`, `test-core-apis.sh`, etc.).
- `config/`: configuration templates; copy `config.template.yaml` to `config.yaml` locally only.
- `docs/`: development/architecture/ops notes (see `docs/development/DEVELOPMENT.md`, `docs/deployment/*`).
- `static/` & `static-dist/`: web assets; `ocr/` stores PaddleOCR models; `runtime/` is generated at run time.

## Build, Test, and Development Commands
- Dev run: `RUST_LOG=debug OCR_CONFIG_PATH=config/config.development.yaml cargo run`.
- Production-style builds: `./scripts/build.sh --prod` (MUSL bundle) or `./scripts/build.sh --prod-native`; Docker path via `./scripts/build-with-docker.sh`.
- Service control: `./scripts/ocr-server.sh start|stop|status`; DM gateway via `./scripts/dm-gateway.sh start`.
- Quality gate: `./scripts/quality-check.sh` (fmt + clippy + check + tests). Use `--fix` to auto-format.
- API smoke: `BASE_URL=http://127.0.0.1:8964 ./scripts/test-core-apis.sh scripts/payloads/preview-20250918.json`.

## Coding Style & Naming Conventions
- Enforce `cargo fmt --all`; Rust 4-space indent; modules/files snake_case, types/traits CamelCase, constants SCREAMING_SNAKE.
- Clippy must be clean; avoid `unwrap`/`expect`, panics, and redundant clones (rules in `clippy.toml`).
- Prefer `?` over manual matching, early returns for error handling, and small public surfaces under `api/` delegating to `util/`/`db/`.
- Keep config and secrets out of code; load via `util/config` with env overrides.

## Testing Guidelines
- Unit tests live beside modules with `#[cfg(test)]` and functions named `test_<behavior>`.
- Run `cargo test --all --no-fail-fast` before pushing; add focused cases for new routes/DB/storage behaviors.
- For integration/API verification, use `./scripts/test-core-apis.sh` (payload required), `test-preview-log.sh`, or `test-dynamic-worker.sh` when touching worker/queue logic.

## Commit & Pull Request Guidelines
- Commit messages: concise, present tense, semantic prefixes (`feat:`, `fix:`, `docs:`, `chore:`, etc.); emoji prefixes are optional but keep intent obvious.
- Before a PR: run `./scripts/quality-check.sh` and note any skipped steps.
- PR description should state what/why, feature flags used, config/env changes, and test evidence (command outputs or screenshots for static/UI tweaks).
- Link issues, call out risky areas (DB/schema, API shape, config keys), and keep diffs focused; split large refactors.

## Security & Configuration Tips
- Never commit real credentials; only track `config/config.template.yaml`. Generate `config/config.yaml` locally and keep it ignored.
- Prefer env vars for secrets (`DM_GATEWAY_API_KEY`, `OSS_ACCESS_SECRET`, `DB_PASSWORD`); verify deploy targets with `./scripts/validate-production-env.sh`.
- Scrub logs and sample payloads; rotate keys in `scripts/payloads/` when sharing.
