# OCR智能预审系统

基于 Rust 构建的文档预审系统，使用 PaddleOCR 进行文字识别，支持可配置的业务规则评估。

## 技术栈

- Rust 1.70+ / Axum 0.7 / Tokio
- PaddleOCR (ocr-conn crate)
- SQLite (默认) / DM数据库 (通过Go网关)
- OSS / 本地存储 (自动故障转移)
- NATS JetStream (分布式模式)

## 项目结构

```
src/
├── main.rs          # 入口 (server/worker/health-check模式)
├── lib.rs           # 全局状态 CONFIG, CLIENT, OCR_SEMAPHORE
├── api/             # HTTP接口 (preview, auth, files, monitoring...)
├── db/              # 数据库层 (SQLite + DM, 自动故障转移)
├── storage/         # 存储层 (OSS + 本地, 自动故障转移)
├── model/           # 数据模型
└── util/            # 工具模块
    ├── task_queue.rs    # NATS任务队列
    ├── zen/             # OCR处理 & 规则引擎
    └── auth/            # 认证中间件

ocr-conn/            # OCR引擎连接器
config/              # 配置文件
scripts/             # 管理脚本
static/              # 前端资源
```

## 预审流程

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  第三方系统  │────▶│ /api/preview │────▶│  任务队列   │────▶│  OCR处理    │
└─────────────┘     └─────────────┘     └─────────────┘     └─────────────┘
                           │                                       │
                           ▼                                       ▼
                    ┌─────────────┐                         ┌─────────────┐
                    │  材料下载    │                         │  规则评估    │
                    └─────────────┘                         └─────────────┘
                                                                   │
                                                                   ▼
                                                            ┌─────────────┐
                                                            │  生成报告    │
                                                            └─────────────┘
```

**处理步骤：**
1. 接收请求 → 2. 参数校验 → 3. 材料下载 → 4. 入队处理 → 5. OCR识别 → 6. 规则评估 → 7. 生成报告 → 8. 回调通知

## 数据库架构

```
┌─────────────────────────────────────────────────────┐
│              Database Trait (抽象层)                 │
└─────────────────────────────────────────────────────┘
                    │           │
         ┌──────────┘           └──────────┐
         ▼                                 ▼
┌─────────────────┐               ┌─────────────────┐
│     SQLite      │               │   DM Gateway    │
│  (默认/备用)     │               │   (生产环境)     │
└─────────────────┘               └─────────────────┘
                                          │
                                          ▼
                                  ┌─────────────────┐
                                  │   Go Gateway    │──▶ DM数据库
                                  │  (HTTP代理)     │
                                  └─────────────────┘
```

**DM网关说明：** Rust 无原生 DM 驱动，通过 Go 网关 (端口8965) HTTP代理访问。DM不可用时自动切换SQLite。

**主要数据表：** `preview_records`, `preview_requests`, `cached_materials`, `monitor_sessions`

## 存储架构

```
Storage Trait ──┬──▶ 阿里云OSS (生产)
                └──▶ 本地存储 (备用)
```

OSS不可用时自动切换本地存储。存储内容：预审报告、材料缓存、OCR中间结果。

## 部署模式

| 模式 | 说明 |
|------|------|
| standalone | 单机模式 (默认) |
| master | 主节点，接收请求分发任务 |
| worker | 工作节点，处理OCR任务 |

**分布式配置：**
```bash
OCR_DEPLOYMENT_ROLE=master|worker
OCR_DISTRIBUTED_ENABLED=true
OCR_NATS_URL=nats://host:4222
```

## 核心接口

| 接口 | 说明 |
|------|------|
| `POST /api/preview` | 提交预审请求 |
| `GET /api/preview/data/:id` | 获取预审结果 |
| `GET /api/health/details` | 详细健康检查 |
| `GET /api/queue/status` | 任务队列状态 |

**认证：** 需要认证的接口添加 `?monitor_session_id=xxx` 参数

## 快速开始

```bash
# 编译
cargo build --release

# 启动
./scripts/ocr-server.sh start

# 健康检查
curl http://localhost:8964/api/health
```

## 文档

- [BUILD.md](./BUILD.md) - 编译打包指南
- [OPERATIONS.md](./OPERATIONS.md) - 运维手册
