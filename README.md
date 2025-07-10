# 智能预审系统 v1.3.0

基于 Rust + Axum + PaddleOCR 的企业级智能文档预审系统

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![Build](https://img.shields.io/badge/build-musl--static-green.svg)](build-release-package.sh)

## ✨ 系统特性

- 🔍 **多语言OCR**: 支持中英日韩俄文识别，基于PaddleOCR引擎
- 🎯 **智能规则**: zen-engine驱动的业务规则引擎  
- 🎨 **主题化预览**: 6套主题规则，支持不同业务场景
- 🔐 **企业级认证**: SSO单点登录 + AK/SK第三方认证
- 📊 **实时监控**: 集成监控系统，性能指标可视化
- 🚀 **高性能**: 异步架构，支持高并发处理
- 📦 **静态部署**: musl编译，无依赖部署

## 🚀 快速开始

### 开发环境
```bash
# 启动服务
./ocr-server.sh start

# 访问页面
http://localhost:31101
```

### 生产部署
```bash
# 构建生产环境包（推荐）
./build-release-package.sh

# 构建调试包（包含开发工具）
./build-simple-package.sh

# 离线部署包
./build-offline-package.sh
```

## 📁 目录结构

### 开发环境
```
ocr-server-src/              # 开发根目录
├── src/                     # Rust源代码
├── config/                  # 配置文件
│   ├── rules/              # 主题规则文件
│   └── matter-theme-mapping.json
├── static/                 # 前端静态文件
├── scripts/               # 管理脚本
└── docs/                  # 文档
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

## 📖 详细文档

- [API参考文档](docs/API_REFERENCE.md)
- [测试指南](docs/TEST_GUIDE.md)
- [部署总结](docs/DEPLOYMENT-SUMMARY.md)
- [模拟登录指南](docs/MOCK_LOGIN_GUIDE.md)
- [系统设计分析](docs/SYSTEM_DESIGN_ANALYSIS.md)

## 🛠️ 开发指南

### 构建选项
```bash
# 开发构建
cargo build

# 生产构建（静态链接）
cargo build --release --target x86_64-unknown-linux-musl

# 或使用封装脚本
./build-and-deploy.sh build
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
- 监控页面: http://localhost:31101/static/monitor.html
- 演示页面: http://localhost:31101/static/demo/