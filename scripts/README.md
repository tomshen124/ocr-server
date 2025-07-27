# 必要脚本说明

## 保留的3个核心脚本

### 1. build.sh - 统一构建脚本
```bash
./build.sh dev           # 开发模式编译
./build.sh prod          # 生产模式编译  
./build.sh release -p    # 发布模式，创建发布包
```

### 2. ocr-server.sh - 服务管理脚本
```bash
./ocr-server.sh start    # 启动服务
./ocr-server.sh stop     # 停止服务
./ocr-server.sh restart  # 重启服务
./ocr-server.sh status   # 查看状态
./ocr-server.sh log      # 查看日志
./ocr-server.sh build    # 构建项目
```

### 3. init.sh - 环境初始化脚本
```bash
./init.sh                # 完整初始化
./init.sh --check        # 仅检查环境
./init.sh --help         # 查看帮助
```

## 使用流程

1. **新环境部署**：
   ```bash
   ./init.sh           # 初始化环境
   ./build.sh prod     # 构建生产版本
   ./ocr-server.sh start  # 启动服务
   ```

2. **开发测试**：
   ```bash
   ./build.sh dev      # 构建开发版本
   ./ocr-server.sh start  # 启动服务
   ```

3. **生产部署**：
   ```bash
   ./build.sh release -p  # 创建发布包
   # 部署发布包后：
   ./start.sh          # 启动服务
   ```

所有功能已集中到这三个脚本中，无需其他冗余工具。