# OCR Server

A high-performance OCR (Optical Character Recognition) server built with Rust, designed for document processing and intelligent preview systems.

## Features

- **High Performance**: Built with Rust for optimal performance and memory safety
- **Modular Architecture**: Clean separation of concerns with well-defined modules
- **OCR Integration**: Support for multiple OCR engines including PaddleOCR
- **Web Interface**: Modern web-based interface for document preview and management
- **Authentication**: SSO integration and third-party access control
- **Storage Flexibility**: Support for both local and cloud storage with failover
- **Database Support**: Multiple database backends with automatic failover
- **Monitoring**: Built-in monitoring and health check endpoints

## Quick Start

### Prerequisites

- Rust 1.70+ 
- OCR engine (PaddleOCR recommended)

### Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd ocr-server
```

2. Copy configuration template:
```bash
cp config.yaml.example config.yaml
```

3. Edit `config.yaml` with your settings

4. Build and run:
```bash
cargo build --release
cargo run --release
```

The server will start on `http://localhost:31101` by default.

## Configuration

See `config.yaml.example` for all available configuration options including:

- Service endpoints and ports
- Database connections
- Storage backends
- Authentication providers
- OCR engine settings
- Monitoring and logging

## API Endpoints

- `GET /` - Web interface
- `POST /api/preview` - Document preview
- `GET /api/monitor` - System monitoring
- `POST /api/auth/*` - Authentication endpoints

## Development

### Project Structure

```
src/
├── api/          # API route handlers
├── db/           # Database abstraction layer
├── model/        # Data models
├── server/       # Server configuration and setup
├── storage/      # Storage backends
└── util/         # Utility modules
    ├── auth/     # Authentication utilities
    ├── config/   # Configuration management
    ├── report/   # Report generation
    └── zen/      # OCR processing utilities
```

### Building

```bash
# Development build
cargo build

# Release build
cargo build --release

# Run tests
cargo test
```

## License

[Add your license information here]