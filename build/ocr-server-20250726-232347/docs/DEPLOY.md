# 部署指南

## 快速部署

### 1. 构建
```bash
./scripts/build.sh -m prod -t musl -p
```

### 2. 部署
```bash
# 上传到服务器
scp releases/ocr-server-*.tar.gz user@server:/tmp/

# 解压并启动
ssh user@server '
  cd /opt && 
  tar -xzf /tmp/ocr-server-*.tar.gz &&
  cd ocr-server-* &&
  ./scripts/ocr-server.sh start
'
```

### 3. 验证
```bash
curl http://localhost:31101/api/health
```

## 配置文件

主要配置文件：`config.yaml`

```yaml
# 基本配置
host: "http://127.0.0.1"
port: 31101

# 数据库（空值使用SQLite）
DMSql:
  DATABASE_HOST: ""

# 对象存储（空值使用本地存储）
zhzwdt-oss:
  AccessKey: ""

# 测试模式
test_mode:
  enabled: false
  auto_login: true

# 故障转移
failover:
  database:
    enabled: true
  storage:
    enabled: true
```

## 服务管理

```bash
# 启动
./scripts/ocr-server.sh start

# 停止
./scripts/ocr-server.sh stop

# 重启
./scripts/ocr-server.sh restart

# 查看状态
./scripts/ocr-server.sh status

# 查看日志
./scripts/ocr-server.sh log
```

## 目录结构

```
ocr-server/
├── bin/ocr-server          # 主程序
├── config/                 # 配置文件
├── static/                 # 前端资源
├── scripts/                # 管理脚本
├── runtime/                # 运行时文件
└── docs/                   # 文档
```

## 环境要求

- Linux x86_64
- 端口31101可用
- 1GB磁盘空间
- （可选）wkhtmltopdf用于PDF生成
