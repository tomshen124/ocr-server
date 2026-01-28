# 编译打包指南

## 本地编译

```bash
# 开发构建
cargo build

# 发布构建
cargo build --release

# 带特性构建
cargo build --release --features "dm_go,monitoring"

# 代码检查
cargo clippy
cargo fmt
./scripts/quality-check.sh
```

## 容器内编译

用于生成 Linux/amd64 二进制：

```bash
# 1. 进入编译容器
docker run --rm -it \
  --platform linux/amd64 \
  -v "$(pwd)":/workspace \
  -w /workspace \
  rust:1.82 \
  bash

# 2. 容器内执行
rustup toolchain install nightly
CARGO_TOOLCHAIN=nightly ./scripts/build.sh --prod-native -f monitoring,dm_go
exit
```

## Docker 镜像构建

```bash
# 标准构建
docker build --platform=linux/amd64 \
  -f docker/Dockerfile \
  -t ocr-server:latest \
  .

# 带特性构建
docker build --platform=linux/amd64 \
  -f docker/Dockerfile \
  -t ocr-server:latest \
  --build-arg RUST_FEATURES="dm_go,monitoring" \
  .

# 无缓存构建
docker build --no-cache --platform=linux/amd64 \
  -f docker/Dockerfile \
  -t ocr-server:latest \
  .
```

## 前端资源构建（可选）

Dockerfile 已内置，通常无需手动执行：

```bash
docker run --rm \
  -v "$(pwd)":/workspace \
  -w /workspace/build-tools \
  node:18-bullseye \
  bash -lc "npm ci && npm run build:prod"
```

## 离线部署包

```bash
./scripts/package-offline.sh v1.4.0
```

## 完整流程

```bash
# 1. 容器内编译
docker run --rm -it --platform linux/amd64 -v "$(pwd)":/workspace -w /workspace rust:1.82 bash
rustup toolchain install nightly
CARGO_TOOLCHAIN=nightly ./scripts/build.sh --prod-native -f monitoring,dm_go
exit

# 2. 构建镜像
docker build --platform=linux/amd64 -f docker/Dockerfile -t ocr-server:latest .

# 3. 生成离线包
./scripts/package-offline.sh v1.4.0
```

## 相关脚本

| 脚本 | 用途 |
|------|------|
| `scripts/build.sh` | 统一编译脚本 |
| `scripts/build-with-docker.sh` | Docker内编译 |
| `scripts/package-offline.sh` | 离线包生成 |
| `scripts/quality-check.sh` | 代码质量检查 |
