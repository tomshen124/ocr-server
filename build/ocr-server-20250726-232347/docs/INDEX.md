# 文档索引

## 📚 核心文档

- [系统概述](SYSTEM_OVERVIEW.md) - 完整的系统介绍和业务流程 ⭐
- [架构设计](ARCHITECTURE.md) - 详细的系统架构和技术设计 ⭐
- [README.md](../README.md) - 项目概述和快速开始
- [API.md](API.md) - API接口文档
- [DEPLOY.md](DEPLOY.md) - 部署指南
- [CONFIG.md](CONFIG.md) - 配置说明
- [TESTING.md](TESTING.md) - 测试指南和用例

## 🔧 管理脚本

- [服务管理](../scripts/ocr-server.sh) - 启动/停止/重启服务
- [构建脚本](../scripts/build.sh) - 编译和打包
- [初始化脚本](../scripts/init.sh) - 环境初始化

## ⚙️ 配置文件

- [主配置](../config.yaml) - 服务配置
- [主题配置](../themes.json) - 业务规则主题
- [事项映射](../matter-theme-mapping.json) - 事项到主题的映射

## 🚀 快速开始

**新用户推荐阅读顺序**:
1. [系统概述](SYSTEM_OVERVIEW.md) - 了解整体架构和业务流程
2. [架构设计](ARCHITECTURE.md) - 理解技术架构和设计思路
3. [部署文档](DEPLOY.md) - 启动和运行系统
4. [测试文档](TESTING.md) - 验证系统功能

**开发人员**:
1. 阅读 [系统概述](SYSTEM_OVERVIEW.md) 了解业务
2. 查看 [架构设计](ARCHITECTURE.md) 了解技术架构
3. 参考 [API.md](API.md) 了解接口设计
4. 运行 `./scripts/ocr-server.sh start` 启动服务
5. 访问 http://localhost:31101/static/test-tools.html 进行测试

**运维人员**:
1. 查看 [DEPLOY.md](DEPLOY.md) 了解部署要求
2. 参考 [CONFIG.md](CONFIG.md) 配置服务参数
3. 使用 `./scripts/ocr-server.sh` 管理服务生命周期

## 📋 支持的政务事项

系统当前支持以下6个真实政务事项的智能预审：
- 工程渣土准运证核准
- 工程渣土消纳场地登记
- 排水接管技术审查
- 临时占用、挖掘河道设施许可
- 设置其他户外广告设施和招牌、指示牌备案
- 利用广场等公共场所举办文化、商业等活动许可

---

**最后更新**: 2024-07-25