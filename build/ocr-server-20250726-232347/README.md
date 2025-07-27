# OCR智能预审系统

版本: 20250726
编译模式: release
目标平台: x86_64-unknown-linux-musl

## 快速开始

1. 配置系统
   - 编辑 config/config.yaml
   - 或使用环境变量覆盖配置

2. 启动服务
   ```bash
   ./start.sh
   ```

3. 检查服务
   - 访问: http://localhost:31101/api/health
   - 查看日志: runtime/logs/

## 目录结构

- bin/         二进制文件
- config/      配置文件
- scripts/     管理脚本
- static/      前端资源
- runtime/     运行时数据
- docs/        文档

