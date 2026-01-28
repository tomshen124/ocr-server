# 运维手册

## 服务管理

```bash
./scripts/ocr-server.sh start    # 启动
./scripts/ocr-server.sh stop     # 停止
./scripts/ocr-server.sh restart  # 重启
./scripts/ocr-server.sh status   # 状态
```

## API快速查看

```bash
# 设置环境变量
export OCR_SERVER="https://sgbzwdt.hzxh.gov.cn:9443"
export OCR_SESSION="你的monitor_session_id"

# 公开接口
./scripts/quick-api.sh health      # 健康检查
./scripts/quick-api.sh queue       # 队列状态
./scripts/quick-api.sh stats       # 调用统计
./scripts/quick-api.sh worker      # Worker状态
./scripts/quick-api.sh failover    # 故障转移状态

# 需要session的接口
./scripts/quick-api.sh data <id>   # 预审数据
./scripts/quick-api.sh status <id> # 预审状态
./scripts/quick-api.sh records 50  # 预审记录
./scripts/quick-api.sh failures    # 失败记录
./scripts/quick-api.sh monitoring  # 系统状态
./scripts/quick-api.sh last        # 最近一次预审
```

## 压力测试

```bash
# 发送测试请求
./scripts/stress-test-preview.sh 1 https://server:port

# 指定样例(1-10)
SAMPLE_NUM=1 ./scripts/stress-test-preview.sh 5 https://server:port

# 测试数据: yushenqingqiu.txt
```

## 常用接口

### 公开接口

| 接口 | 说明 |
|------|------|
| `GET /api/health` | 健康检查 |
| `GET /api/health/details` | 详细健康 |
| `GET /api/queue/status` | 队列状态 |
| `GET /api/stats/calls` | 调用统计 |

### 需要认证的接口

添加 `?monitor_session_id=xxx`：

| 接口 | 说明 |
|------|------|
| `POST /api/preview` | 提交预审 |
| `GET /api/preview/data/:id` | 预审数据 |
| `GET /api/preview/download/:id` | 下载报告 |
| `GET /api/preview/records` | 记录列表 |
| `GET /api/preview/failures` | 失败记录 |
| `GET /api/monitoring/status` | 系统状态 |

## 日志位置

```
runtime/logs/           # 主日志
runtime/logs/requests/  # 请求日志
runtime/data/ocr.db     # 数据库
```

## 问题排查

```bash
# 检查服务
./scripts/quick-api.sh health

# 检查队列
./scripts/quick-api.sh queue

# 查看失败
./scripts/quick-api.sh failures

# 查看特定预审
./scripts/quick-api.sh data PREVIEW_ID
```

## 脚本清单

| 脚本 | 用途 |
|------|------|
| `ocr-server.sh` | 服务管理 |
| `quick-api.sh` | API查看 |
| `stress-test-preview.sh` | 压力测试 |
| `build.sh` | 编译 |
| `quality-check.sh` | 代码检查 |
| `package-offline.sh` | 离线打包 |
| `cluster-manager.sh` | 集群管理 |
| `start-worker.sh` | Worker启动 |
