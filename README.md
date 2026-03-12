# OCR Server

A lightweight OCR API server for extracting text from images and integrating OCR into automation workflows.

Built with Rust and Axum, this repository packages OCR upload APIs, document preview/report generation, health monitoring, and standalone or worker-based deployment modes for production-style document processing pipelines.

> Status: archived snapshot of a production-oriented service. The codebase remains useful as a deployable reference implementation and integration base.

## Overview

`ocr-server` is designed for teams that need OCR as a backend capability rather than a desktop tool. It exposes HTTP endpoints for OCR uploads and preview workflows, stores generated artifacts locally or in object storage, and includes health/monitoring routes that make it suitable for automation, internal platforms, and AI-assisted document pipelines.

This project is designed to support AI-powered automation workflows and developer tools.

## Features

- HTTP OCR endpoint for images and PDFs
- JSON response envelope for straightforward integration
- Preview/report workflow with HTML and PDF outputs
- Health, monitoring, queue, and failover endpoints
- Standalone, master, worker, and hybrid deployment roles
- Bundled OCR runtime assets under [`ocr/`](./ocr)
- Config templates and production build script under [`scripts/build.sh`](./scripts/build.sh)

## Installation

### Prerequisites

- Rust toolchain
- `wkhtmltopdf` for PDF report export
- Linux/macOS environment capable of running the bundled OCR runtime libraries

### Build

```bash
cargo build --release
./scripts/build.sh --prod-native
./scripts/build.sh --prod
```

## Usage

### Local Run

```bash
cp config/config.template.yaml config/config.yaml
./target/release/ocr-server
```

Default service address:

- Base URL: `http://127.0.0.1:8964`
- Health endpoint: `GET /api/health`

### CLI Health Check

```bash
./target/release/ocr-server health-check
```

### Runtime Configuration

The service reads `config/config.yaml` when present and supports environment overrides such as:

- `OCR_HOST`
- `OCR_PORT`
- `OCR_DEPLOYMENT_ROLE`
- `OCR_NATS_URL`
- `DB_PASSWORD`
- `DM_GATEWAY_API_KEY`
- `OSS_ACCESS_SECRET`

See [`config/config.template.yaml`](./config/config.template.yaml) for the full template.

## API Example

### Health Check

```bash
curl http://127.0.0.1:8964/api/health
```

### Preview Workflow Request

Use the sample payload in [`examples/preview-request.json`](./examples/preview-request.json):

```bash
curl -X POST \
  http://127.0.0.1:8964/api/preview \
  -H 'Content-Type: application/json' \
  --data @examples/preview-request.json
```

### OCR Upload Request

The repository includes a sample image at [`examples/test.png`](./examples/test.png):

```bash
curl -X POST \
  http://127.0.0.1:8964/api/upload \
  -F "file=@examples/test.png"
```

In the current server layout, `/api/upload` is part of the authenticated application routes.

Additional endpoint notes are documented in [`docs/api.md`](./docs/api.md).

## Docker Deployment

Build the container:

```bash
docker build -t ocr-server .
```

Run the container:

```bash
docker run --rm \
  -p 8964:8964 \
  -e OCR_HOST=0.0.0.0 \
  -e OCR_PORT=8964 \
  ocr-server
```

The container image includes the Rust binary, bundled OCR assets, static frontend files, and `wkhtmltopdf`.

## Use Cases

- OCR microservice for internal automation platforms
- Document ingestion pipelines that need text extraction over HTTP
- AI workflow backends that need OCR before downstream classification or summarization
- Pre-review and attachment-processing systems that generate traceable reports

## Repository Layout

```text
ocr-server-src/
├── src/                 Rust application code
├── ocr/                 Bundled OCR runtime and models
├── config/              Config templates
├── static/              Frontend and monitoring assets
├── examples/            Sample request payloads and test asset
├── docs/                API notes
├── scripts/build.sh     Production-oriented build helper
└── Dockerfile           Container build for deployment
```

## Roadmap

- [ ] Add a lightweight public demo configuration
- [ ] Add batch OCR examples for automation pipelines
- [ ] Add container publishing workflow
- [ ] Add GPU-oriented deployment notes
- [ ] Add CLI helper commands for common preview operations

## Contributing

Pull requests are welcome. See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the expected workflow.

## License

This project is available under the [`MIT License`](./LICENSE).
