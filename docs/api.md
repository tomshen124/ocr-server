# API Notes

This document summarizes the reviewer-visible API surface for local evaluation and integration planning.

## Base URL

`http://127.0.0.1:8964`

## Response Envelope

Most JSON responses use this structure:

```json
{
  "success": true,
  "errorCode": 200,
  "errorMsg": "",
  "data": {}
}
```

## Endpoints

### `GET /api/health`

Basic service health probe.

Example:

```bash
curl http://127.0.0.1:8964/api/health
```

### `GET /api/health/details`

Detailed health information for service components.

### `GET /api/health/components`

Component-level health summary.

### `POST /api/preview`

Creates a preview/OCR processing task from structured document metadata and attachment URLs.

Input:

- JSON body
- See [`examples/preview-request.json`](/Users/xiaopang/ocr-server-src/examples/preview-request.json)
- Main fields: `agentInfo`, `subjectInfo`, `materialData`, `matterId`, `matterName`, `matterType`, `sequenceNo`

Auth behavior:

- Controlled by `third_party_access` config and `OCR_FORCE_THIRD_PARTY_AUTH`
- Depending on configuration, the endpoint may allow open access, access-key identification, or signed third-party authentication

Example:

```bash
curl -X POST \
  http://127.0.0.1:8964/api/preview \
  -H 'Content-Type: application/json' \
  --data @examples/preview-request.json
```

### `POST /api/upload`

Performs OCR on an uploaded image or PDF and returns extracted text fragments.

Input:

- `multipart/form-data`
- field name: `file`

Example:

```bash
curl -X POST \
  http://127.0.0.1:8964/api/upload \
  -F "file=@examples/test.png"
```

Notes:

- PDF uploads are constrained by the configured file size and page limits.
- This route is registered under authenticated application routes in the current server configuration.

### `GET /api/preview/result/:preview_id`

Returns structured preview/OCR result data for an existing preview record.

### `GET /api/preview/download/:preview_id`

Downloads a generated preview report.

### Monitoring and Ops

The repository also exposes operational endpoints such as:

- `GET /api/monitoring/status`
- `GET /api/queue/status`
- `GET /api/resources/status`
- `GET /api/failover/status`
- `GET /api/stats/calls`

## Example Assets

- Sample OCR input image: [`examples/test.png`](/Users/xiaopang/ocr-server-src/examples/test.png)
- Sample preview request body: [`examples/preview-request.json`](/Users/xiaopang/ocr-server-src/examples/preview-request.json)
