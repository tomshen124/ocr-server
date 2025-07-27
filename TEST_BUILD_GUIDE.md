# OCR智能预审系统 - 测试规整与生产编译指南

## 测试规整环境设置

### 1. 配置环境检测
系统会自动检测当前环境并加载对应配置：

- **开发环境**: 使用项目根目录的 `config.yaml`
- **测试规整**: 使用 `config.yaml.prod` 覆盖默认配置
- **生产环境**: 使用 `config/config.yaml` 相对二进制路径

### 2. 测试规整专用配置

创建 `config.yaml.prod` 文件进行测试规整配置：

```yaml
# 测试规整配置示例
debug:
  enabled: false              # 关闭调试模式
  enable_mock_login: false    # 禁用模拟登录
  tools_enabled: false        # 禁用调试工具

third_party_access:
  enabled: true               # 启用第三方访问控制
  signature:
    required: true           # 要求请求签名
  rate_limit:
    requests_per_minute: 60  # 测试环境限速

logging:
  level: "info"              # 测试环境日志级别
  file:
    enabled: true
    retention_days: 7        # 测试环境保留7天日志

# 其他测试规整特定配置...
```

### 3. 测试规整专用构建

使用统一构建脚本进行测试规整构建：

```bash
# 测试规整构建（使用生产模式配合测试配置）
./scripts/build.sh -m prod -t musl

# 或启用监控功能的生产构建
cargo build --release --features monitoring
```

## 生产编译包类型区分

### 1. 编译包类型说明

当前系统支持以下编译包类型：

| 类型 | 命令 | 特点 | 用途 |
|------|------|------|------|
| **开发包** | `-m dev` | 启用调试功能，体积较大 | 本地开发 |
| **生产包** | `-m prod` | 最优化，无调试功能 | 测试/生产环境 |
| **发布包** | `-m release` | 生产包+压缩包 | 正式发布 |

**注意**: 系统支持3种编译模式：`dev`(开发)、`prod`(生产)、`release`(发布)。测试环境请使用 `-m prod` 模式并配合测试配置。

### 2. 生产编译优化

生产编译使用以下优化设置：

```toml
# Cargo.toml 生产配置
[profile.release]
opt-level = 3          # 最高优化级别
lto = "thin"          # 链接时优化
codegen-units = 1      # 单一编译单元
strip = true          # 移除调试符号
panic = "abort"       # 更小二进制文件
```

### 3. musl静态链接编译

生产环境推荐使用musl静态链接：

```bash
# 安装musl目标
rustup target add x86_64-unknown-linux-musl

# Ubuntu/Debian安装musl工具
sudo apt install musl-tools

# CentOS/RHEL安装musl工具
sudo yum install musl-libc musl-libc-devel

# 生产构建（musl静态链接）
./scripts/build.sh -m prod -t musl
```

### 4. 构建命令对比

```bash
# 当前release包（等同prod）
cargo build --release

# 推荐的生产包（musl静态链接）
./scripts/build.sh -m prod -t musl

# 带监控的生产包
./scripts/build.sh -m prod -t musl -f monitoring

# 发布包（包含压缩包）
./scripts/build.sh -m release -t musl -p
```

## 环境部署验证

### 1. 测试规整验证步骤

```bash
# 1. 环境检查
./scripts/check-environment.sh

# 2. 构建测试规整版本（使用生产模式）
./scripts/build.sh -m prod -t musl

# 3. 启动服务
./scripts/ocr-server.sh start

# 4. 验证连接
./scripts/test-connections.sh

# 5. 检查服务状态
./scripts/ocr-server.sh status

# 6. 查看日志
./scripts/ocr-server.sh log
```

### 2. 生产部署验证

```bash
# 1. 生产环境检查
./scripts/check-environment.sh --env prod

# 2. 生产构建
./scripts/build.sh -m prod -t musl -p

# 3. 初始化配置
./scripts/init.sh --env prod

# 4. 启动服务
./scripts/ocr-server.sh start

# 5. 健康检查
curl http://localhost:31101/api/health

# 6. 详细状态检查
curl http://localhost:31101/api/health/details
```

## 配置优先级说明

配置加载优先级（从高到低）：

1. **环境变量** (最高优先级)
2. **config.yaml.prod** (测试规整专用)
3. **config/config.yaml** (生产配置)
4. **config.yaml** (默认配置)
5. **内置默认值** (最低优先级)

## 常见问题解决

### 1. 构建失败
```bash
# 清理构建缓存
./scripts/build.sh -c

# 重新构建
./scripts/build.sh -m prod -t musl
```

### 2. 配置不生效
- 确认 `config.yaml.prod` 文件存在且格式正确
- 检查环境变量是否覆盖了配置文件设置
- 验证配置文件路径是否正确

### 3. 权限问题
```bash
# 修正权限
chmod +x scripts/*.sh
chmod +x build/*.sh

# 修正目录权限
sudo chown -R $USER:$USER runtime/
sudo chown -R $USER:$USER config/
```

## 最佳实践

1. **测试规整**: 使用 `config.yaml.prod` 进行测试环境配置
2. **生产编译**: 始终使用 `-m prod -t musl` 进行生产构建
3. **版本管理**: 为不同环境创建不同的构建包
4. **配置管理**: 使用环境变量覆盖敏感配置
5. **监控**: 生产环境启用监控功能 `-f monitoring`

## 一键部署脚本

创建一键部署脚本 `deploy-test.sh`：

```bash
#!/bin/bash
# 测试规整一键部署

set -e

echo "开始测试规整部署..."

# 1. 环境检查
./scripts/check-environment.sh

# 2. 清理旧版本
./scripts/build.sh -c

# 3. 测试规整构建（使用生产模式）
./scripts/build.sh -m prod -t musl

# 4. 停止旧服务
./scripts/ocr-server.sh stop || true

# 5. 启动新服务
./scripts/ocr-server.sh start

# 6. 验证部署
sleep 5
./scripts/test-connections.sh

# 7. 状态检查
echo "部署完成！"
./scripts/ocr-server.sh status
```

使用方法：
```bash
chmod +x deploy-test.sh
./deploy-test.sh
```