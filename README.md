# OCR智能预审系统 v1.4.0

基于Rust + Axum的高性能OCR预审服务，专为政务数字化转型设计。

> **v1.4.0重要更新**: 基于达梦Go网关（HTTP）的数据库连接架构，智能故障转移机制，完善的监控体系，并发控制优化

## 🚀 快速开始

### 1. 环境要求
- Rust 1.70+
- PaddleOCR环境  
- Docker (推荐生产部署)
- 8GB+ 内存

### 2. 快速部署 (推荐)
```bash
# 🚀 启动DM网关服务
./scripts/dm-gateway.sh start

# 🎯 启动主服务器
./scripts/ocr-server.sh start

# 检查服务状态
curl http://localhost:8964/api/health
curl -X POST -H "X-API-Key: <api-key>" http://localhost:8080/health
```

### 3. 配置说明
```bash
# 主配置文件
ls config/
# config.yaml - 主配置文件（生产不入库，模板见 config/config.template.yaml）

# 设置生产环境变量
export OCR_DEBUG_ENABLED=false
export OCR_RUNTIME_MODE=production
export OCR_HOST=0.0.0.0
export DB_PASSWORD="your_password"
export OSS_ACCESS_KEY="your_access_key"  
export OSS_ACCESS_SECRET="your_secret_key"
export DM_GATEWAY_URL="http://<gw-host>:8080"
export DM_GATEWAY_API_KEY="<api-key>"
```

### 4. 启动服务
```bash
# 开发模式
cargo run

# 生产模式  
./scripts/ocr-server.sh start

# DM网关管理
./scripts/dm-gateway.sh start
```

### 5. 访问服务
- Web界面: http://localhost:8964
- 健康检查: http://localhost:8964/api/health
- 测试工具: http://localhost:8964/static/test-tools.html
- DM网关: http://localhost:8080 (健康检查)

## 📁 核心结构

```
ocr-server-src/
├── src/                        # Rust主服务器
│   ├── api/                   # HTTP API路由
│   ├── model/                 # 数据模型  
│   ├── db/                    # 数据库抽象层
│   │   └── dm/                # 达梦数据库Go网关（HTTP）连接 🆕
│   ├── storage/               # 文件存储抽象层
│   ├── util/                  # 工具模块
│   └── server/                # 服务器启动
├── scripts/                    # 管理脚本
│   ├── ocr-server.sh          # 主服务器管理
│   └── dm-gateway.sh          # 网关管理 🆕
└── docs/                       # 文档
    ├── API.md                 # 接口文档
    ├── PRODUCTION_DEPLOYMENT_GUIDE.md  # 生产部署完整指南
    └── DISTRIBUTED_DEPLOYMENT.md       # 分布式与架构说明
```

## 🔧 主要功能

- ✅ 多格式OCR识别 (PDF/JPG/PNG)
- ✅ 智能预审引擎
- ✅ 事项级规则配置（数据库驱动，JSON Schema，可热更新） 🆕
- ✅ SSO单点登录集成
- ✅ 达梦Go网关（HTTP）数据库连接 🆕
- ✅ 智能故障转移机制 🆕 (数据库+存储双重降级)
- ✅ 并发控制与监控 (12任务并发控制)
- ✅ 完整的健康检查体系 🆕
- ✅ API调用统计与审计 🆕
- ✅ 分布式链路追踪支持 🆕

## 📊 API接口

| 接口 | 方法 | 说明 |
|------|------|------|
| `/api/health` | GET | 健康检查 |
| `/api/preview` | POST | 提交预审请求 |
| `/api/preview/view/{id}` | GET | 查看预审结果 |
| `/api/rules/matters` | GET | 查询所有事项的规则概览 🆕 |
| `/api/rules/matters/{matter_id}` | GET | 查看单个事项的规则详情 🆕 |
| `/api/failover/status` | GET | 故障转移状态 🆕 |
| `/api/monitoring/status` | GET | 系统监控状态 🆕 |
| `/api/queue/status` | GET | 并发队列状态 🆕 |

### 规则配置导入工具

首次部署或批量更新事项规则时，可使用内置的小工具将 `matter-*.json` 导入数据库：

```bash
# 导入当前目录下的所有 matter-*.json
cargo run --bin import_matter_rules -- ./

# 或指定多个目录/文件
cargo run --bin import_matter_rules -- ./rules matter-101104353.json
```

工具会自动读取配置文件、计算校验值并写入 `matter_rule_configs` 表；重复导入同一事项会执行更新。


### DM网关API接口（Go）🆕
| 接口 | 方法 | 说明 |
|------|------|------|
| `http://localhost:8080/health` | POST | 健康检查（需 `X-API-Key`） |
| `http://localhost:8080/db` | POST | SQL查询接口（需 `X-API-Key`） |

## 🛡️ 安全说明

- 不提交明文凭据：仓库仅保留 `config/config.template.yaml`，`config/config.yaml` 应在部署环境生成并被 .gitignore 忽略。
- 使用环境变量管理密钥和密码（DB/OSS/DM 网关等）。
- 生产环境需启用签名验证/限流（`third_party_access`）和严格的 CORS 白名单。
- 日志避免输出敏感信息（API Key/签名等）；生产建议 `logging.level=info`。

更多细节参见：`docs/CONFIGURATION.md` 与 `docs/CODE_REVIEW.md`。

## 📖 详细文档

- [生产部署](docs/PRODUCTION_DEPLOYMENT_GUIDE.md) - 完整的生产部署说明
- [分布式与网络](docs/DISTRIBUTED_DEPLOYMENT.md) - 架构与网络端口说明
- [API文档](docs/API.md) - 所有接口的详细说明
- [开发指南](docs/DEVELOPMENT.md) - 开发环境搭建和调试
- [预审流程与时序](docs/PREVIEW_FLOW.md) - 请求→材料下载→任务→结果的全链路与时序图 🆕
- [完整架构说明](CLAUDE.md) - 系统全面介绍 (v1.3.0)

## 🚢 部署与构建（发布包）

- 一键发布（MUSL 静态，通用部署）
  - `./scripts/build.sh --prod`
  - 如需启用达梦 Go 网关，建议显式：`./scripts/build.sh --prod -f monitoring,dm_go`
- 一键发布（glibc 原生，性能优先）
  - `./scripts/build.sh --prod-native`
  - 同样如需启用达梦 Go 网关：`./scripts/build.sh --prod-native -f monitoring,dm_go`
- 发布包内容：二进制、配置与规则、静态资源、`scripts/ocr-server.sh`，并仅附带精简文档（`README.md`、`docs/API.md`、可选 `docs/DEPLOYMENT.md`）。

## 🔧 环境变量（覆盖配置）

- 基础服务：`OCR_HOST`、`OCR_PORT`、`OCR_DEBUG_ENABLED`、`OCR_RUNTIME_MODE`
- 数据库口令：`DB_PASSWORD`
- 达梦网关（Go）：
  - `DM_GATEWAY_URL` → 覆盖 `database.dm.go_gateway.url`
  - `DM_GATEWAY_API_KEY` → 覆盖 `database.dm.go_gateway.api_key`
- OSS 存储：`OSS_ACCESS_KEY`、`OSS_ACCESS_SECRET`、`OSS_BUCKET`、`OSS_ROOT`
- 代理旁路（专有云 OSS 强烈建议）：
  - `NO_PROXY=localhost,127.0.0.1,<oss-bucket-domain>,<oss-endpoint-domain>`

说明：`config.yaml` 中的 `${DM_GATEWAY_API_KEY}` 仅作为占位符，程序不会自动展开。若需要使用占位，请配合上述环境变量覆盖；或直接在 YAML 中填入真实值（开发/联调场景）。

## 🔄 故障转移（当前行为）

- 数据库（smart 模式）：优先连接达梦（Go 网关），连接失败自动降级至 SQLite，服务不中断。自动回切与数据回灌将按计划分两步上线（先 Outbox 幂等回灌，后熔断策略）。
- 存储：优先 OSS，写入失败自动切换本地存储并完成读写验证；建议为专有云环境设置 `NO_PROXY` 避免被代理拦截导致 503。
- 状态检查：`GET /api/failover/status`、`GET /api/queue/status`。

## 🧪 统一测试脚本

- 统一入口：`./scripts/test-suite.sh --url http://localhost:8964 [quick|api|preview|auth|all]`
  - quick：健康/队列/故障转移冒烟
  - api：核心接口（健康、监控、预审、验证）
  - preview：预审多场景数据
  - auth：第三方调用与回调配置探测

提示：示例材料 URL（example.com）不返回真实文件，OCR 会报解析失败属预期。建议使用 data:base64 或先 `/api/upload` 后引用已上传文件。

## 🔐 监控与认证

- 监控接口 `GET /api/monitoring/status` 需要监控登录，生产环境务必修改默认凭据。
- SSO 调用为开发便捷模式，生产请部署可信 CA 或由反向代理终止 TLS。

---

## 📝 文档变更

- 日期：2025-09-12
- 本次更新：
  - 强化安全说明：配置模板与环境变量；生产不提交明文凭据
  - 新增 `docs/CONFIGURATION.md`，统一配置与覆盖策略说明
  - 链接代码审查报告 `docs/CODE_REVIEW.md`
  - 补充 DM 网关环境变量示例

维护：OCR 项目组

## 🤝 贡献

请查看 [CLAUDE.md](CLAUDE.md) 了解完整的开发规范和架构设计。
