# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 🎯 OCR智能预审系统架构概览

### 技术栈
- **后端**: Rust 1.70+ + Axum + Tokio (异步Web框架)
- **OCR引擎**: PaddleOCR (多语言支持: 中英日韩俄)
- **规则引擎**: zen-engine (业务规则执行)
- **存储**: 阿里云OSS + 本地存储双模式
- **数据库**: SQLite (默认) + 达梦数据库 (企业版)
- **认证**: SSO单点登录 + AK/SK第三方认证
- **监控**: 集成健康检查 + 性能指标收集

### 核心架构
```
src/
├── api/           # HTTP路由层 (Axum) - 🚨 需要重构
│   ├── mod.rs     # 主模块 (2533行，过大！)
│   └── [规划中]   # 模块化拆分计划见下文
├── model/         # 数据模型和OCR处理
│   ├── ocr.rs     # OCR识别核心
│   ├── preview.rs # 预审逻辑
│   ├── evaluation.rs # 评估引擎
│   └── user.rs    # 用户管理
├── util/          # 工具函数和中间件
│   ├── config.rs  # 配置管理
│   ├── zen.rs     # 规则引擎
│   ├── test_mode.rs # 测试模式
│   └── third_party_auth.rs # 第三方认证
├── db/            # 数据库抽象层
│   ├── sqlite.rs  # SQLite实现
│   ├── failover.rs # 故障转移
│   └── factory.rs # 数据库工厂
├── storage/       # 存储抽象层
│   ├── local.rs   # 本地存储
│   ├── oss.rs     # OSS存储
│   └── failover.rs # 存储故障转移
└── monitor/       # 监控和健康检查模块
    ├── mod.rs     # 模块定义 (feature-gated)
    ├── service.rs # 监控服务核心
    ├── health.rs  # 健康检查器
    ├── metrics.rs # 性能指标数据结构
    ├── api.rs     # 监控API路由
    └── config.rs  # 监控配置
```

### 🚨 API模块重构计划 (优先级: 高)

**问题**: `src/api/mod.rs` 文件过大 (2533行, 50个函数)，违反单一职责原则

**计划架构**:
```
src/api/
├── mod.rs          # 路由组装 (~100行)
├── auth.rs         # 认证API (~400行)
├── preview.rs      # 预审核心API (~800行) 
├── upload.rs       # 文件处理API (~300行)
├── monitoring.rs   # 监控统计API (~400行)
├── config.rs       # 配置管理API (~200行)
└── utils.rs        # 共享工具函数 (~300行)
```

**重构优先级**:
1. 🔴 `preview.rs` - 核心业务功能
2. 🔴 `auth.rs` - 安全认证功能  
3. 🟡 `monitoring.rs` - 新增并发监控
4. 🟢 其他模块 - 代码质量改善

**详细计划**: 参见 `docs/API_REFACTOR_PLAN.md`

## 🚀 快速开发命令

### 环境启动
```bash
# 一键启动开发环境
./scripts/ocr-server.sh start

# 测试环境启动
./scripts/test.sh test localhost 31101

# 生产环境验证
./scripts/test.sh prod
```

### 构建系统
```bash
# 开发模式快速编译
./scripts/build.sh -m dev

# 生产环境静态编译
./scripts/build.sh -m prod -t musl

# 发布包创建
./scripts/build.sh -m release -t musl -p

# 清理缓存
./scripts/build.sh -c
```

## 🔧 统一前端架构 (2025年7月更新)

### 环境切换机制
- **配置驱动**: 通过 `config.yaml` 中的 `debug.enabled` 控制模式
- **测试模式**: `debug.enabled: true` + 测试参数
- **生产模式**: `debug.enabled: false`

### 核心文件
```
static/
├── index.html             # 主页面 (统一入口)
├── monitor.html           # 📊 预审监控统计页面 (NEW 2025-07-27)
├── monitoring.html        # 📊 系统监控仪表板页面 (NEW)
├── stats.html             # 📈 统计分析页面 (NEW)
├── test-tools.html        # 🧪 基础测试工具页面
├── test-tools-gov.html    # 🧪 政务测试工具页面 (NEW)
├── css/
│   ├── main.css           # 主要样式文件
│   ├── components.css     # 组件样式
│   └── modals.css         # 模态框样式
├── js/
│   ├── config.js          # 🆕 API配置和数据映射 (NEW 2025-07-27)
│   ├── api.js             # 🔄 API服务层 (重构 2025-07-27)
│   ├── app.js             # 🔄 主应用逻辑 (重构 2025-07-27)
│   ├── components.js      # UI组件库
│   └── utils.js           # 工具函数
└── images/                # UI图片资源
```

### 🆕 新增前端功能 (2025年7月更新)

#### 1. 🚀 预审监控统计系统 (monitor.html) - 2025-07-27

**核心功能**:
- **请求监控**: 完整的预审请求生命周期跟踪
- **ID映射**: 第三方请求ID ↔ 系统预审ID 双向映射
- **事项统计**: 按事项名称(matterName)和事项ID(matterId)分类统计
- **时间跟踪**: 请求时间、预审完成时间、处理时长计算
- **状态管理**: pending/processing/completed/failed 全状态覆盖

**数据展示**:
```
预审ID | 第三方请求ID | 事项名称 | 事项ID | 用户ID | 状态 | 请求时间 | 完成时间 | 处理时长 | 操作
```

**过滤和查询**:
- 📅 日期范围过滤 (开始日期 - 结束日期)
- 🏷️ 状态过滤 (全部状态/待处理/处理中/已完成/处理失败)
- 🔍 事项名称搜索
- 📄 分页显示 (每页20条记录)
- 📊 实时统计卡片 (总数/成功/处理中/失败)

**操作功能**:
- 👁️ 查看预审结果 (跳转到预审页面)
- 📥 下载预审报告 (PDF格式)
- 📤 导出数据 (CSV格式)
- 🔄 30秒自动刷新统计数据

**API集成**:
```
GET /api/preview/statistics    # 获取预审统计数据
GET /api/preview/records       # 获取预审记录列表(支持分页和过滤)
```

#### 2. 监控仪表板 (monitoring.html)
- **实时监控**: 系统CPU、内存、磁盘使用率
- **OCR服务状态**: 服务运行状态、响应时间监控
- **性能趋势图**: 动态性能数据可视化
- **健康检查**: 组件健康状态详细展示
- **自动刷新**: 30秒间隔自动数据更新
- **系统日志**: 实时日志显示和管理

##### 主要功能模块
```javascript
// monitoring.html 核心功能
- refreshSystemStatus()     # 刷新系统状态
- refreshHealthCheck()      # 健康检查
- refreshPerformanceData()  # 性能数据更新
- updateSystemMetrics()     # 系统指标显示
- startAutoRefresh()        # 自动刷新控制
```

#### 2. 统计分析页面 (stats.html)
- **处理量统计**: 总处理量、今日处理量、成功率
- **用户活动**: 活跃用户统计和活动记录
- **主题使用统计**: 各主题使用频率分析
- **趋势分析**: 多时间维度数据分析 (今日/本周/本月/本年)
- **数据导出**: 统计报表导出功能
- **实时更新**: 5分钟间隔数据刷新

##### 统计功能模块
```javascript
// stats.html 核心功能
- loadOverviewStats()       # 加载概览统计
- loadThemeStats()          # 主题使用统计
- loadUserStats()           # 用户活动统计
- loadTrendData()           # 趋势数据分析
- changeTimeRange()         # 时间范围切换
- exportThemeStats()        # 导出统计报表
```

#### 3. 政务测试工具 (test-tools-gov.html)
- **模拟登录测试**: 用户身份模拟和会话管理
- **预审数据测试**: 支持模拟/真实/混合数据模式
- **第三方接口测试**: qingqiu.json数据格式验证
- **系统状态监控**: 集成系统状态检查
- **政务风格界面**: 符合政务系统设计规范

##### 测试功能模块
```javascript
// test-tools-gov.html 核心功能
- performMockLogin()        # 执行模拟登录
- testPreviewData()         # 预审数据测试
- testThirdPartyCallback()  # 第三方回调测试
- refreshSystemStatus()     # 系统状态刷新
- validateThirdPartyData()  # 数据格式验证
```

## 📁 目录结构详解

### 开发环境
```
ocr-server-src/
├── src/                     # Rust源代码
│   ├── api/mod.rs          # 路由定义 src/api/mod.rs:21
│   ├── model/              # 数据模型
│   ├── util/               # 工具函数
│   ├── db/                 # 数据库抽象
│   ├── storage/            # 存储抽象
│   └── main.rs             # 入口文件 src/main.rs:214
├── config/                 # 配置文件
│   ├── rules/              # 主题规则 (JSON格式)
│   ├── mappings/           # 映射配置目录
│   │   └── matter-theme-mapping.json
│   ├── themes.json         # 主题定义
│   ├── config.yaml         # 主配置文件
│   ├── config.development.yaml # 开发环境配置
│   ├── config.production.yaml  # 生产环境配置
│   └── config.yaml.template    # 配置模板
├── static/                 # 前端统一文件
├── scripts/                # 管理脚本
└── ocr/                    # PaddleOCR引擎
```

## 📊 监控系统架构 (Monitor Module)

### 核心组件
```
src/monitor/
├── mod.rs          # 模块定义，支持feature-gated编译
├── service.rs      # 监控服务核心实现
├── health.rs       # OCR服务健康检查器
├── metrics.rs      # 系统和OCR指标数据结构
├── api.rs          # 监控API路由定义
└── config.rs       # 监控配置管理
```

### 监控功能特性
- **系统监控**: CPU、内存、磁盘使用率实时监控
- **OCR服务监控**: 端口监听、API响应、进程状态检查
- **健康检查**: 多层次健康状态检测 (端口/API/进程)
- **指标历史**: 24小时性能数据历史记录
- **告警机制**: 资源使用率超阈值自动告警
- **API集成**: RESTful监控接口集成

### 监控API端点
```
GET  /api/monitoring/status     # 获取系统状态概览
GET  /api/monitoring/health     # OCR服务健康检查
GET  /api/monitoring/history    # 获取历史性能数据
GET  /api/health/details        # 详细健康信息
GET  /api/health/components     # 组件健康状态
```

### 前端监控界面
- **monitoring.html**: 实时监控仪表板
- **stats.html**: 统计分析和报表页面
- **集成导航**: 统一的前端监控入口
```

### 生产部署包
```
ocr-server-{version}/
├── bin/ocr-server          # 可执行文件
├── config/                 # 配置文件
├── static/                 # 前端资源
├── runtime/                # 运行时数据
│   ├── logs/              # 日志文件
│   └── preview/           # 预览结果
└── scripts/               # 管理脚本
```

## 🔄 核心功能模块

### 1. OCR处理流程
- **输入**: PDF/JPG/PNG/BMP文件
- **处理**: PaddleOCR多语言识别
- **输出**: 结构化JSON + HTML预览
- **存储**: OSS/本地双模式自动切换

### 2. 主题规则系统
- **位置**: `config/rules/theme_*.json`
- **机制**: zen-engine规则引擎
- **映射**: matterId/matterName自动匹配
- **热更新**: `/api/themes/{theme_id}/reload`

### 3. 认证系统
- **SSO**: 第三方单点登录
- **AK/SK**: API密钥认证
- **测试**: 模拟登录 (开发模式)
- **状态**: Session-based用户管理

### 4. 🚀 并发控制系统 (2025-07-27 新增)
- **信号量控制**: 全局OCR_SEMAPHORE限制并发任务
- **智能排队**: 系统繁忙时自动排队，避免拒绝服务
- **资源优化**: 针对32核64G服务器的12任务并发限制
- **实时监控**: `/api/queue/status` 提供队列状态查询
- **前端集成**: 页面顶部实时显示系统负载状态

**核心实现**:
```rust
// src/main.rs - 全局信号量
pub static OCR_SEMAPHORE: LazyLock<Arc<Semaphore>> = LazyLock::new(|| {
    Arc::new(Semaphore::new(12)) // 32核服务器的保守并发设置
});

// src/api/mod.rs - 预审函数中的并发控制
let permit = OCR_SEMAPHORE.acquire().await?;
// ... OCR处理逻辑 ...
// permit自动释放
```

**性能指标**:
- **最大并发**: 12个OCR任务同时处理
- **内存控制**: 预计48-72GB使用 (每任务4-6GB)
- **CPU利用率**: 保持在70-85%安全范围
- **响应时间**: 立即返回提交状态，透明排队处理

### 5. 高可用架构
- **数据库故障转移**: SQLite降级
- **存储故障转移**: 本地存储降级
- **智能恢复**: 服务恢复后自动同步
- **健康检查**: `/api/health/details`

## ⚙️ 配置管理

### 配置文件优先级
1. 环境变量 (`OCR_CONFIG_PATH`, `OCR_TEST_MODE`)
2. 配置文件 (`config/config.yaml`)
3. 开发环境配置 (`config/config.development.yaml`)
4. 生产环境配置 (`config/config.production.yaml`)
5. 默认配置

### 关键配置项
```yaml
# config/config.yaml - 完整配置结构
# ============= 基本服务配置 =============
host: "http://127.0.0.1"
port: 31101
preview_url: "http://127.0.0.1:31101"
session_timeout: 86400
app_id: "2002387292"
callback_url: "http://127.0.0.1:31101/api/sso/callback"

# ============= 单点登录配置 =============
login:
  sso_login_url: "https://ibcdsg.zj.gov.cn:8443/restapi/..."
  access_token_url: "https://ibcdsg.zj.gov.cn:8443/restapi/..."
  get_user_info_url: "https://ibcdsg.zj.gov.cn:8443/restapi/..."
  access_key: "BCDSGA_xxx"
  secret_key: "BCDSGS_xxx"
  use_callback: false

# ============= 对象存储配置 =============
zhzwdt-oss:
  root: "ocr-files"
  bucket: "your-bucket"
  server_url: "https://your-oss-endpoint.com"
  AccessKey: ""  # 空值使用本地存储
  "AccessKey Secret": ""

# ============= 数据库配置 =============
DMSql:
  DATABASE_HOST: ""  # 空值使用SQLite
  DATABASE_PORT: "5236"
  DATABASE_USER: "SYSDBA"
  DATABASE_PASSWORD: "SYSDBA"
  DATABASE_NAME: "OCR_DB"

# ============= 开发调试配置 =============
debug:
  enabled: true  # 生产环境设为false
  enable_mock_login: true
  mock_login_warning: true
  tools_enabled:
    api_test: true
    system_monitor: true
    flow_test: true

# ============= 故障转移配置 =============
failover:
  database:
    enabled: true
    fallback_to_sqlite: true
    recovery_check_interval: 300
  storage:
    enabled: true
    fallback_to_local: true
    local_fallback_path: "./runtime/storage"
```

## 🧪 测试验证

### ⚠️ **当前测试问题及解决方案**

#### 主要问题
1. **完整流程测试不完整**: 现有测试脚本只做基础健康检查，缺乏端到端预审流程验证
2. **前后端集成测试缺失**: 前端页面与后端API集成存在gap，特别是预审结果展示环节
3. **认证流程测试复杂**: SSO/模拟登录/第三方认证多种方式混杂，测试覆盖不完整
4. **测试工具分散**: 功能齐全但分散在不同页面，缺乏统一的测试入口

#### 推荐测试方案
```bash
# 1. 基础服务测试
./scripts/test.sh test localhost 31101

# 2. 🚀 并发控制测试 (新增 2025-07-27)
./test-concurrency.sh  # 验证12任务并发限制

# 3. 📊 生产环境验证 (新增 2025-07-27)  
./production-verify.sh  # 完整部署验证

# 4. 手动完整流程测试 (推荐)
# 访问测试工具页面进行端到端测试
http://localhost:31101/static/test-tools.html

# 5. 快速流程验证
./quick-test.sh  # 包含认证+预审完整流程

# 6. 生产环境验证
./scripts/test.sh prod
```

### 功能测试
```bash
# 快速验证所有功能
./scripts/test.sh test

# 生产环境验证
./scripts/test.sh prod

# 指定服务器验证
./scripts/test.sh test 192.168.1.100 31101
```

### 🔧 **完整流程测试指南**

#### 测试环境准备
1. **启动服务**: `./scripts/ocr-server.sh start`
2. **验证健康**: `curl http://localhost:31101/api/health`
3. **检查配置**: 确认`config.yaml`中的测试模式已启用

#### 端到端测试步骤
1. **访问测试工具**: `http://localhost:31101/static/test-tools.html`
2. **模拟登录测试**: 验证用户认证流程
3. **文件上传测试**: 上传PDF/图片文件进行OCR识别
4. **预审流程测试**: 验证主题匹配、规则引擎、结果生成
5. **结果展示测试**: 检查预审结果页面渲染和数据完整性

#### 测试工具页面功能
- **基础API测试**: 健康检查、主题列表、认证状态
- **模拟登录测试**: 支持自定义用户ID和姓名
- **预审数据测试**: 支持模拟/真实/混合数据模式
- **完整流程测试**: 端到端预审流程自动化验证
- **第三方接口测试**: qingqiu.json格式验证和回调测试
- **系统状态监控**: 实时系统健康状态显示

### API测试端点
- **健康检查**: `GET /api/health`
- **主题列表**: `GET /api/themes`
- **认证状态**: `GET /api/auth/status`
- **调试工具**: `GET /static/test-tools.html` (主要测试入口)
- **系统监控**: `GET /static/monitoring.html`
- **预审监控**: `GET /static/monitor.html` (NEW 2025-07-27)
- **统计分析**: `GET /static/stats.html`

### 🆕 预审监控API端点 (2025-07-27)
- **预审统计**: `GET /api/preview/statistics` - 获取预审处理统计数据
- **预审记录**: `GET /api/preview/records` - 获取预审记录列表(支持分页和过滤)
  - 查询参数: `page`, `size`, `status`, `date_from`, `date_to`, `matter_name`
- **预审数据**: `GET /api/preview/data/{previewId}` - 获取指定预审的详细数据

## 🔄 前后端集成重构 (2025-07-27)

### 架构优化
1. **移除Mock数据**: 前端完全移除模拟数据，改为调用真实后端API
2. **配置化管理**: 新增 `static/js/config.js` 统一管理API端点和数据映射
3. **API服务重构**: `static/js/api.js` 支持配置化端点和数据转换
4. **错误处理**: 改进前端错误处理和降级机制

### 数据映射配置
```javascript
// config.js - API配置示例
const API_CONFIG = {
    endpoints: {
        previewData: '/preview/data/{previewId}',
        previewStatistics: '/preview/statistics',
        previewRecords: '/preview/records'
    }
};

const DATA_MAPPING = {
    preview: {
        basicInfo: {
            applicant: ['applicant_name', 'legalRep.FDDBR', 'self.DWMC'],
            applicationType: ['matter_name', 'application_type']
        }
    }
};
```

### 前端组件更新
- **ApiService**: 支持配置化端点和数据转换
- **IntelligentAuditApp**: 移除Mock数据依赖，真实API集成
- **错误处理**: 统一错误消息和降级处理机制

### 测试验证
完整的端到端测试流程：
1. 模拟登录 → 2. 第三方数据提交 → 3. 预审处理 → 4. 结果展示 → 5. 监控统计

## 🛠️ 常见开发任务

### 添加新主题
1. 创建 `config/rules/theme_XXX.json`
2. 更新 `config/matter-theme-mapping.json`
3. 热重载: `POST /api/themes/theme_XXX/reload`

### 调试新功能
```bash
# 开发模式测试
./scripts/build.sh -m dev
./scripts/ocr-server.sh start

# 访问调试工具
http://localhost:31101/static/debug/index-new.html
```

### 性能测试
```bash
# 基准测试
./scripts/test.sh test --performance

# 压力测试 (需Apache Bench)
ab -n 100 -c 10 http://localhost:31101/api/health
```

## 🔍 调试工具

### 内置调试接口
- **模拟登录**: `POST /api/dev/mock_login`
- **测试数据**: `POST /api/test/mock/data`
- **日志统计**: `GET /api/logs/stats`
- **系统监控**: `GET /api/health/details`

### 🆕 增强调试功能 (2025年7月更新)

#### 政务测试工具页面 (test-tools-gov.html)
- **模拟登录测试**: 支持用户ID/姓名自定义模拟登录
- **认证状态检查**: 实时检查当前用户认证状态
- **会话管理**: 清除会话、状态重置功能
- **预审数据测试**: 支持模拟/真实/混合三种数据模式
- **完整流程测试**: 端到端预审流程验证
- **第三方接口测试**: qingqiu.json格式验证和回调测试
- **系统状态监控**: 集成系统健康状态实时显示

#### 统计分析工具 (stats.html)
- **实时数据统计**: 处理量、成功率、用户活动统计
- **主题使用分析**: 各OCR主题使用频率和成功率统计
- **用户行为分析**: 用户活动记录和使用模式分析
- **趋势分析图表**: 多时间维度数据可视化
- **数据导出功能**: 统计报表导出 (开发中)

#### 监控仪表板 (monitoring.html)
- **实时系统监控**: CPU/内存/磁盘使用率监控
- **OCR服务状态**: 服务健康状态和响应时间监控
- **性能趋势图表**: 系统性能历史数据可视化
- **自动刷新机制**: 30秒间隔自动数据更新
- **系统日志显示**: 实时系统日志查看和管理

### 调试模式配置
```yaml
# config.yaml 调试相关配置
debug:
  enabled: true                    # 开启调试模式
  enable_mock_login: true          # 启用模拟登录
  mock_login_warning: true         # 显示模拟登录警告
  tools_enabled:                   # 启用的调试工具
    api_test: true                 # API测试工具
    system_monitor: true           # 系统监控工具
    flow_test: true                # 流程测试工具
    stats_analysis: true           # 统计分析工具
```

### 环境变量
```bash
# 启用测试模式
export OCR_TEST_MODE=true

# 日志级别
export RUST_LOG=debug

# 自定义配置路径
export OCR_CONFIG_PATH=/path/to/config.yaml
```

## 📊 监控和日志

### 日志文件
- **应用日志**: `runtime/logs/ocr-*.log`
- **访问日志**: `runtime/logs/access-*.log`
- **错误日志**: `runtime/logs/error-*.log`

### 监控指标
- **系统健康**: `/api/health/details`
- **组件状态**: `/api/health/components`
- **性能指标**: `/api/logs/stats`
- **实时监控**: `/static/monitoring.html`
- **统计分析**: `/static/stats.html`
- **测试工具**: `/static/test-tools-gov.html`

### 📊 监控系统集成指南

#### 启用监控功能
```rust
// Cargo.toml 中启用monitoring feature
[features]
default = ["monitoring"]
monitoring = []

// 代码中条件编译
#[cfg(feature = "monitoring")]
pub mod monitor;
```

#### 监控配置
```yaml
# config.yaml 监控相关配置
monitoring:
  enabled: true
  check_interval: 60        # 检查间隔(秒)
  history_retention: 1440   # 历史数据保留时间(分钟)
  alert_thresholds:
    cpu_usage: 90.0         # CPU告警阈值
    memory_usage: 90.0      # 内存告警阈值
    disk_usage: 90.0        # 磁盘告警阈值
    ocr_memory_mb: 500      # OCR进程内存阈值
```

#### 监控API使用示例
```bash
# 获取系统状态概览
curl http://localhost:31101/api/monitoring/status

# 检查OCR服务健康状态
curl http://localhost:31101/api/monitoring/health

# 获取性能历史数据
curl http://localhost:31101/api/monitoring/history

# 获取详细健康信息
curl http://localhost:31101/api/health/details

# 获取组件健康状态
curl http://localhost:31101/api/health/components
```

#### 前端页面导航
```
主页面 (/) 
├── 预审页面 (/preview.html)
├── 监控中心 (/monitoring.html)     # 📊 实时监控仪表板
├── 统计中心 (/stats.html)          # 📈 数据统计分析  
└── 测试工具 (/test-tools-gov.html) # 🧪 开发测试工具
```

## 🚀 部署指南

### 本地开发
```bash
# 1. 初始化环境
./scripts/init.sh

# 2. 启动服务
./scripts/ocr-server.sh start

# 3. 访问测试
http://localhost:31101/login.html?test=1
```

### 生产部署
```bash
# 1. 构建发布包
./scripts/build.sh -m release -t musl -p

# 2. 解压部署
tar -xzf ocr-server-*.tar.gz
cd ocr-server-*

# 3. 启动服务
./start.sh
```

### Docker部署 (可选)
```dockerfile
FROM rust:1.70-alpine AS builder
WORKDIR /app
COPY . .
RUN ./scripts/build.sh -m prod -t musl

FROM alpine:latest
COPY --from=builder /app/build/ocr-server /usr/local/bin/
EXPOSE 31101
CMD ["ocr-server"]
```

## 🚨 重要提醒

### 安全性
- **生产环境**: 关闭调试模式 (`debug.enabled: false`)
- **认证配置**: 正确配置SSO参数
- **文件权限**: 确保runtime目录可写
- **网络访问**: 配置防火墙规则

### 性能优化
- **静态编译**: 使用musl target减少依赖
- **缓存策略**: 合理配置OCR结果缓存
- **资源限制**: 监控内存和CPU使用
- **日志轮转**: 配置日志清理策略

### 故障排查
```bash
# 查看服务状态
./scripts/ocr-server.sh status

# 查看实时日志
./scripts/ocr-server.sh log

# 健康检查
./scripts/test.sh test localhost 31101

# 构建验证
./scripts/build.sh -m dev
```

## 🔧 API模块重构计划 (2025-07-27)

### ⚠️ 重构必要性
当前 `src/api/mod.rs` 文件存在严重问题：
- **文件过大**: 2533行代码，超出合理维护范围
- **职责混杂**: 认证、预审、上传、下载、监控等50个函数混合
- **维护困难**: 单一文件难以多人协作开发
- **测试复杂**: 功能耦合导致单元测试困难

### 🎯 重构方案
```
src/api/
├── mod.rs              # 模块声明和路由组装 (~100行)
├── auth.rs             # 认证相关API (~400行)
├── preview.rs          # 预审核心API (~800行)  
├── upload.rs           # 文件处理API (~300行)
├── monitoring.rs       # 监控统计API (~400行)
├── config.rs           # 配置管理API (~200行)
└── utils.rs            # 共享工具函数 (~300行)
```

### 🚨 重构优先级
1. **🔴 高优先级**: `preview.rs` (核心业务) → `auth.rs` (安全关键)
2. **🟡 中优先级**: `monitoring.rs` (新增并发控制) → `upload.rs` 
3. **🟢 低优先级**: `config.rs` → `utils.rs` → `mod.rs`

### 📋 重构执行计划
1. **预备阶段**: 创建功能分支，备份原文件
2. **核心拆分**: 按优先级依次提取各模块
3. **路由重组**: 重新组织mod.rs的路由结构
4. **测试验证**: 确保所有API功能正常
5. **文档更新**: 同步更新开发文档

详细重构计划参见: `docs/API_REFACTOR_PLAN.md`

### ⚡ 并发控制优化 (已实施)
- **全局信号量**: 限制最大12个并发OCR处理任务
- **队列监控**: 新增 `/api/queue/status` 端点
- **前端状态**: 实时显示系统负载和处理状态
- **性能保障**: 64GB/32核服务器下的稳定运行