# Scripts 脚本说明

## 编译相关

| 脚本 | 用途 |
|------|------|
| `build.sh` | 统一编译脚本 |
| `build-with-docker.sh` | Docker容器内编译 |
| `package-offline.sh` | 生成离线部署包 |
| `quality-check.sh` | 代码质量检查 (clippy/fmt) |
| `pre-commit.sh` | Git pre-commit hook |

## 运维相关

| 脚本 | 用途 |
|------|------|
| `ocr-server.sh` | 服务管理 (start/stop/restart/status) |
| `quick-api.sh` | API快速查看工具 |
| `stress-test-preview.sh` | 预审压力测试 |

## 使用示例

```bash
# 编译
./scripts/build.sh
cargo build --release

# 服务管理
./scripts/ocr-server.sh start
./scripts/ocr-server.sh status

# API查看
./scripts/quick-api.sh health
./scripts/quick-api.sh last

# 压力测试
./scripts/stress-test-preview.sh 1 https://server:port
```

---
更新时间: 2026-01-16
