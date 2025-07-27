# 🚀 OCR智能预审系统 - 生产环境快速上线指南

## 📋 部署概览

**服务器配置**: 64GB内存 + 32核CPU + 单OCR服务实例
**目标**: 快速上线，解决核心问题，保证系统稳定运行

---

## ⚡ 立即修复清单 (上线前必做)

### 🔴 Critical - 必须修复

#### 1. 安全问题修复
```bash
# 检查并修复所有unwrap调用
grep -r "\.unwrap()" src/ --include="*.rs"
grep -r "\.expect(" src/ --include="*.rs"
```

**关键文件需要修复**:
- `src/main.rs:146` - database port解析
- `src/api/mod.rs` - 多处unwrap调用
- `src/util/config.rs` - 配置文件读取

#### 2. 并发处理限制 (核心优化)
```rust
// 在main.rs中添加全局并发限制
use std::sync::Arc;
use tokio::sync::Semaphore;

// 根据32核CPU，建议并发限制为16-24个OCR任务
pub static OCR_SEMAPHORE: LazyLock<Arc<Semaphore>> = LazyLock::new(|| {
    Arc::new(Semaphore::new(20)) // 并发OCR任务限制
});

// 在preview函数中使用
let _permit = OCR_SEMAPHORE.acquire().await?;
// OCR处理逻辑...
```

#### 3. 内存使用监控
```rust
// 在util/system_info.rs中添加内存监控
pub fn check_memory_usage() -> bool {
    let usage = get_memory_usage();
    if usage.usage_percent > 80.0 {
        tracing::warn!("内存使用率过高: {}%", usage.usage_percent);
        return false;
    }
    true
}
```

---

## 🐳 容器化部署 (推荐)

### Dockerfile优化
```dockerfile
FROM rust:1.70-alpine AS builder
WORKDIR /app
COPY . .
# 静态编译，减少运行时依赖
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM alpine:latest
RUN apk add --no-cache ca-certificates tzdata
# 设置时区
ENV TZ=Asia/Shanghai
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/ocr-server /usr/local/bin/
COPY config/ /app/config/
COPY static/ /app/static/
WORKDIR /app
EXPOSE 31101
CMD ["ocr-server"]
```

### Docker Compose部署
```yaml
version: '3.8'
services:
  ocr-server:
    build: .
    ports:
      - "31101:31101"
    volumes:
      - ./config:/app/config
      - ./runtime:/app/runtime
      - ./logs:/app/logs
    environment:
      - RUST_LOG=info
      - OCR_CONFIG_PATH=/app/config/config.yaml
    deploy:
      resources:
        limits:
          memory: 32G
          cpus: '16'
        reservations:
          memory: 16G
          cpus: '8'
    restart: unless-stopped
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:31101/api/health"]
      interval: 30s
      timeout: 10s
      retries: 3
```

---

## ⚙️ 生产环境配置

### config/config.production.yaml
```yaml
# 生产环境配置
host: "http://your-production-domain.com"
port: 31101
session_timeout: 86400

# 生产环境日志配置
logging:
  level: "info"
  file:
    enabled: true
    directory: "/app/logs"
    max_size: "100MB"
    retention_days: 30
  console:
    enabled: false

# 监控配置
monitoring:
  enabled: true
  check_interval: 60
  history_retention: 1440
  alert_thresholds:
    cpu_usage: 85.0      # 32核CPU，85%告警
    memory_usage: 80.0   # 64GB内存，80%告警  
    disk_usage: 90.0
    ocr_memory_mb: 2048  # 单个OCR任务内存限制

# 故障转移配置
failover:
  database:
    enabled: true
    fallback_to_sqlite: true
    recovery_check_interval: 300
  storage:
    enabled: true
    fallback_to_local: true
    local_fallback_path: "/app/runtime/storage"

# 关闭调试模式
debug:
  enabled: false
  enable_mock_login: false
  mock_login_warning: false
```

---

## 📊 性能优化配置

### 1. 操作系统调优
```bash
# /etc/sysctl.conf 系统参数优化
# 网络连接优化
net.core.somaxconn = 65535
net.core.netdev_max_backlog = 5000
net.ipv4.tcp_max_syn_backlog = 65535

# 内存管理优化
vm.swappiness = 10
vm.dirty_ratio = 15
vm.dirty_background_ratio = 5

# 文件描述符限制
fs.file-max = 1000000

# 应用到系统
sysctl -p
```

### 2. 服务启动脚本
```bash
#!/bin/bash
# start-ocr-server.sh

# 设置环境变量
export RUST_LOG=info
export RUST_BACKTRACE=1
export OCR_CONFIG_PATH=/app/config/config.production.yaml

# 设置进程限制
ulimit -n 65535
ulimit -u 32768

# 启动服务
exec /usr/local/bin/ocr-server
```

---

## 🔍 监控与告警

### 1. 健康检查脚本
```bash
#!/bin/bash
# health-check.sh

HEALTH_URL="http://localhost:31101/api/health"
RESPONSE=$(curl -s -o /dev/null -w "%{http_code}" $HEALTH_URL)

if [ $RESPONSE -eq 200 ]; then
    echo "OCR服务正常"
    exit 0
else
    echo "OCR服务异常: HTTP $RESPONSE"
    exit 1
fi
```

### 2. 日志监控
```bash
# 实时监控关键日志
tail -f /app/logs/ocr-*.log | grep -E "(ERROR|WARN|预审任务失败)"

# 内存使用监控
watch -n 30 'free -h && ps aux | grep ocr-server | grep -v grep'
```

---

## 🚨 生产环境应急处理

### 常见问题快速排查

#### 1. 内存不足
```bash
# 检查内存使用
free -h
ps aux --sort=-%mem | head -10

# 如果内存使用过高，重启服务
docker-compose restart ocr-server
```

#### 2. CPU使用率过高
```bash
# 检查CPU使用
top -p $(pgrep ocr-server)

# 检查OCR任务积压
curl -s http://localhost:31101/api/health/details | jq '.queue'
```

#### 3. 磁盘空间不足
```bash
# 清理日志文件
find /app/logs -name "*.log" -mtime +7 -delete

# 清理临时文件
find /app/runtime -name "*.tmp" -mtime +1 -delete
```

---

## 📈 性能基准测试

### 压力测试脚本
```bash
#!/bin/bash
# performance-test.sh

# 并发测试OCR接口
for i in {1..10}; do
  curl -X POST http://localhost:31101/api/preview \
    -H "Content-Type: application/json" \
    -d @qingqiu.json &
done

wait
echo "并发测试完成"
```

---

## 🔧 快速故障恢复

### 1. 服务重启脚本
```bash
#!/bin/bash
# restart-service.sh

echo "正在重启OCR服务..."
docker-compose down
sleep 5
docker-compose up -d
echo "服务重启完成"

# 等待服务启动
sleep 30
./health-check.sh
```

### 2. 数据备份脚本
```bash
#!/bin/bash
# backup-data.sh

DATE=$(date +%Y%m%d_%H%M%S)
BACKUP_DIR="/backup/ocr-$DATE"

mkdir -p $BACKUP_DIR
cp -r /app/runtime/data $BACKUP_DIR/
cp -r /app/config $BACKUP_DIR/

echo "数据备份完成: $BACKUP_DIR"
```

---

## 📋 上线检查清单

### 部署前检查
- [ ] 修复所有unwrap()调用
- [ ] 添加OCR并发限制
- [ ] 配置生产环境参数
- [ ] 准备监控脚本
- [ ] 测试故障恢复流程

### 部署后检查
- [ ] 健康检查通过
- [ ] 日志正常输出
- [ ] 内存使用率 < 80%
- [ ] CPU使用率 < 85%
- [ ] 并发处理测试通过

### 运行监控
- [ ] 设置定时健康检查
- [ ] 配置日志轮转
- [ ] 设置告警阈值
- [ ] 建立应急响应流程

---

## ⚡ 立即可用的性能优化

### 1. 环境变量优化
```bash
# 针对32核CPU的Tokio优化
export TOKIO_WORKER_THREADS=16
export RAYON_NUM_THREADS=16

# Rust编译器优化
export RUSTFLAGS="-C target-cpu=native"
```

### 2. 系统资源限制
```bash
# /etc/security/limits.conf
ocr-user soft nofile 65535
ocr-user hard nofile 65535
ocr-user soft nproc 32768
ocr-user hard nproc 32768
```

---

## 📞 应急联系和支持

### 关键指标监控
- **内存使用**: 保持在 < 80% (51.2GB)
- **CPU使用**: 保持在 < 85% (27核)
- **并发OCR**: 限制在20个任务
- **响应时间**: < 30秒/请求

### 紧急情况处理
1. 服务无响应 → 重启服务
2. 内存不足 → 清理临时文件 + 重启
3. 磁盘满 → 清理日志文件
4. CPU过高 → 检查OCR任务积压

---

**部署时间预估**: 2-4小时
**测试验证**: 1-2小时  
**总上线时间**: 4-6小时

⚠️ **重要提醒**: 务必在生产环境部署前在测试环境完整验证所有功能！