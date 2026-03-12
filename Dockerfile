FROM rust:1.85-bookworm AS builder

WORKDIR /build
COPY . .
RUN cargo build --release --bin ocr-server

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        wkhtmltopdf \
        libstdc++6 \
        libglib2.0-0 \
        libx11-6 \
        libxext6 \
        libxrender1 \
        libfontconfig1 \
        libjpeg62-turbo \
        zlib1g \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/ocr-server /app/ocr-server
COPY --from=builder /build/config /app/config
COPY --from=builder /build/ocr /app/ocr
COPY --from=builder /build/static /app/static

RUN mkdir -p /app/runtime/logs /app/runtime/data /app/preview /app/images /app/storage/previews \
    && cp /app/config/config.template.yaml /app/config/config.yaml

EXPOSE 8964

ENV OCR_HOST=0.0.0.0
ENV OCR_PORT=8964

CMD ["./ocr-server"]
