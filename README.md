# 智能预审系统 v2.0

基于 Rust + Axum + PaddleOCR 的企业级智能文档预审系统

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![Build](https://img.shields.io/badge/build-musl--static-green.svg)](build-release-package.sh)
[![Platform](https://img.shields.io/badge/platform-linux-green.svg)](https://www.linux.org/)

## ✨ 系统特性

- 🔍 **多语言OCR**: 支持中英日韩俄文识别，基于PaddleOCR引擎
- 🎯 **智能规则**: zen-engine驱动的业务规则引擎  
- 🎨 **主题化预览**: 6套主题规则，支持不同业务场景
- 🔐 **企业级认证**: SSO单点登录 + AK/SK第三方认证
- 📊 **实时监控**: 集成监控系统，性能指标可视化
- 🚀 **高性能**: 异步架构，支持高并发处理
- 📦 **静态部署**: musl编译，无依赖部署
- 🛡️ **高可用架构**: 数据库和存储故障自动降级

## 🚀 快速开始

### 一键初始化（推荐）
```bash
# 克隆项目
git clone https://github.com/your-org/ocr-server.git
cd ocr-server

# 运行初始化向导
./scripts/init.sh

# 启动开发服务
./start-dev.sh
```

### 手动启动
```bash
# 启动服务
./scripts/ocr-server.sh start

# 访问页面
http://localhost:31101

# 查看统计仪表板
http://localhost:31101/static/statistics.html
```

### 生产部署
```bash
# 构建生产环境包（推荐）
./scripts/build.sh -m prod -t musl -p

# 构建调试包（包含开发工具）
./scripts/build.sh -m dev

# 创建离线部署包
./build-offline-package.sh
```

## 📁 目录结构

### 开发环境
```
ocr-server-src/              # 开发根目录
├── src/                     # Rust源代码
│   ├── api/                # API接口层
│   ├── db/                 # 数据库抽象层
│   ├── storage/            # 存储抽象层
│   ├── model/              # 数据模型
│   └── util/               # 工具函数
├── config/                 # 配置文件
│   ├── rules/              # 主题规则文件
│   ├── config.*.yaml       # 环境配置
│   └── matter-theme-mapping.json
├── static/                 # 前端静态文件
│   ├── css/               # 样式文件
│   ├── js/                # JavaScript
│   └── statistics.html    # 统计仪表板
├── scripts/               # 管理脚本
│   ├── init.sh           # 初始化脚本
│   ├── build.sh          # 统一编译脚本
│   ├── config-manager.sh # 配置管理
│   └── check-environment.sh # 环境检查
└── docs/                  # 项目文档
```

### 生产环境（发布包）
```
ocr-server-{version}/       # 生产根目录
├── bin/                    # 应用程序
│   └── ocr-server
├── config/                 # 配置文件
│   ├── config.yaml
│   ├── rules/             # 规则文件
│   └── matter-theme-mapping.json
├── static/                # 前端资源
├── scripts/               # 管理脚本
│   ├── ocr-server.sh      # 核心服务管理
│   ├── test_api.sh        # API测试
│   └── ...
├── runtime/               # 运行时文件
│   ├── logs/             # 日志
│   └── preview/          # 预览结果
├── docs/                  # 文档
└── ocr-server            # 统一管理入口
```

## 🔧 核心功能

- **OCR文字识别**: 基于PaddleOCR引擎
- **智能预览**: 支持PDF生成和结构化数据提取
- **多主题规则**: 6个主题规则支持不同业务场景
- **自动映射**: matterId/matterName自动映射到对应主题
- **Web界面**: 现代化的前端操作界面

## 📖 文档

- [API接口文档](docs/API.md) - 核心接口说明
- [部署指南](docs/DEPLOY.md) - 快速部署
- [配置说明](docs/CONFIG.md) - 配置参考
- [文档索引](docs/INDEX.md) - 完整文档导航

## 🛠️ 开发指南

### 环境初始化
```bash
# 运行初始化向导
./scripts/init.sh

# 手动环境检查
./scripts/check-environment.sh
```

### 配置管理
```bash
# 验证配置文件
./scripts/config-manager.sh validate

# 切换环境配置
./scripts/config-manager.sh switch development
./scripts/config-manager.sh switch production

# 生成环境变量文件
./scripts/config-manager.sh generate-env > .env
```

### 构建选项
```bash
# 使用统一构建脚本
./scripts/build.sh -m dev              # 开发模式
./scripts/build.sh -m prod -t musl     # 生产模式（静态链接）
./scripts/build.sh -m release -p       # 发布模式（创建部署包）

# 或使用cargo直接构建
cargo build
cargo build --release --target x86_64-unknown-linux-musl
```

### 配置管理
- 主配置: `config/config.yaml`
- 规则文件: `config/rules/theme_*.json`
- 映射配置: `config/matter-theme-mapping.json`

### 服务管理
```bash
# 开发环境
./ocr-server.sh {start|stop|restart|status|log}

# 生产环境
./ocr-server start    # 启动服务
./ocr-server stop     # 停止服务  
./ocr-server status   # 查看状态
./ocr-server restart  # 重启服务
./ocr-server log      # 查看日志
```

## 🌟 主要特性

1. **规整的目录结构**: 生产包采用标准化目录布局
2. **智能路径检测**: 脚本自动适应开发/生产环境
3. **向后兼容**: 保持对旧版本部署的兼容性
4. **简化部署**: 一键构建、打包、部署流程
5. **多平台支持**: 支持CentOS、Ubuntu、Debian等主流Linux发行版

## 📝 版本说明

- **简化包**: 不包含wkhtmltopdf，需要手动安装（推荐）
- **完整包**: 包含wkhtmltopdf离线安装包，网络下载

## 🔗 相关链接

- 服务地址: http://localhost:31101
- 统计仪表板: http://localhost:31101/static/statistics.html
- 监控页面: http://localhost:31101/static/monitor.html
- 演示页面: http://localhost:31101/static/demo/

## 🆕 v2.0 新增功能

### 高可用架构
- **数据库故障转移**: 自动降级到本地SQLite
- **存储故障转移**: OSS不可用时自动切换到本地存储
- **智能恢复**: 服务恢复后自动切回并同步数据

### 统计仪表板
- 实时监控系统运行状态
- API调用统计和分析
- 系统资源使用情况
- 预审趋势图表展示

### 增强的管理工具
- **初始化向导**: `./scripts/init.sh`
- **统一编译脚本**: `./scripts/build.sh`
- **配置管理器**: `./scripts/config-manager.sh`
- **环境检查**: `./scripts/check-environment.sh`

### 改进的前端体验
- 纯HTML+CSS+JS实现，无框架依赖
- 响应式设计，支持移动端
- 实时数据更新
- 优雅的加载动画

## 🤝 贡献指南

欢迎贡献代码、报告问题或提出建议！

1. Fork 项目
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 创建 Pull Request

## 📄 许可证

本项目采用 MIT 许可证 - 查看 [LICENSE](LICENSE) 文件了解详情

---

<p align="center">
  Built with ❤️ by OCR Team
</p>