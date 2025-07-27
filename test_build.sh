#!/bin/bash
set -x
echo "Starting build..."
cargo build --release --target x86_64-unknown-linux-musl
echo "Build completed with exit code: $?"